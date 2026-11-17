//! Windows에서 압축파일 안 항목을 창 밖으로 끌어 놓기.
//!
//! 셸이 만든 데이터 객체(SHCreateDataObject)와 기본 끌기 원본(SHDoDragDrop)을 써서
//! 탐색기에서 파일을 끄는 것과 같은 형식(CF_HDROP, 셸 항목 목록, 끌기 이미지)으로 넘김.
//! 셸 끌기는 시작할 때 파일이 있어야 하므로 앱이 먼저 풀고, 다 풀렸을 때 버튼을 아직
//! 누르고 있으면 끌기를 시작함. 끌기는 놓거나 취소할 때까지 이 스레드에서 진행됨.

use std::{collections::VecDeque, ffi::c_void, path::PathBuf};
use windows_sys::{
    Win32::{
        Foundation::{DRAGDROP_S_DROP, HWND},
        System::Ole::{DROPEFFECT, DROPEFFECT_COPY, OleInitialize},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
            Shell::{
                Common::ITEMIDLIST, ILFindLastID, ILFree, SHCreateDataObject, SHDoDragDrop,
                SHParseDisplayName,
            },
            WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON},
        },
    },
    core::GUID,
};
use zipelo_ui_memory_app::drag_out::{DragEvent, DragOut};

/// IID_IDataObject {0000010e-0000-0000-C000-000000000046}
const IID_IDATAOBJECT: GUID = GUID {
    data1: 0x0000_010e,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

pub struct WindowsDrag {
    hwnd: HWND,
    paths: Vec<PathBuf>,
    events: VecDeque<DragEvent>,
}

/// 창이 Win32 창이면 끌어 놓기 구현을 만듦.
pub fn create(handles: &impl raw_window_handle::HasWindowHandle) -> Option<WindowsDrag> {
    let raw_window_handle::RawWindowHandle::Win32(window) = handles.window_handle().ok()?.as_raw()
    else {
        return None;
    };
    zipelo_ui_memory_app::log_debug!("Windows 끌어 놓기 준비");
    Some(WindowsDrag {
        hwnd: window.hwnd.get() as HWND,
        paths: Vec::new(),
        events: VecDeque::new(),
    })
}

fn wide(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let text = path.as_os_str().encode_wide().collect::<Vec<_>>();
    // 셸 경로 해석은 \\?\ 접두사를 받지 않으므로 뗌.
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let text = if text.starts_with(&prefix) {
        text[prefix.len()..].to_vec()
    } else {
        text
    };
    text.into_iter().chain(Some(0)).collect()
}

/// 셸 항목 목록(PIDL). 버릴 때 해제함.
struct Pidl(*mut ITEMIDLIST);

impl Pidl {
    fn parse(path: &std::path::Path) -> Option<Self> {
        let name = wide(path);
        let mut pidl = std::ptr::null_mut();
        // 안전성: name은 NUL로 끝나는 UTF-16이고 pidl은 결과를 받을 지역 변수임.
        let result = unsafe {
            SHParseDisplayName(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut pidl,
                0,
                std::ptr::null_mut(),
            )
        };
        (result >= 0 && !pidl.is_null()).then_some(Self(pidl))
    }
}

impl Drop for Pidl {
    fn drop(&mut self) {
        // 안전성: SHParseDisplayName이 할당한 PIDL을 한 번만 해제함.
        unsafe { ILFree(self.0) };
    }
}

impl DragOut for WindowsDrag {
    fn after_extract(&self) -> bool {
        true
    }

    fn button_held(&mut self) -> bool {
        // 안전성: 인자 없는 Win32 상태 조회임.
        unsafe {
            let swapped = GetSystemMetrics(SM_SWAPBUTTON) != 0;
            let key = if swapped { VK_RBUTTON } else { VK_LBUTTON };
            GetAsyncKeyState(key as i32) < 0
        }
    }

    fn start(&mut self) -> bool {
        let Some(parent) = self.paths.first().and_then(|path| path.parent()) else {
            return false;
        };
        // 셸 데이터 객체는 같은 폴더의 항목만 담을 수 있음. 목록의 한 폴더에서 고른 항목임.
        if self.paths.iter().any(|path| path.parent() != Some(parent)) {
            zipelo_ui_memory_app::log_warn!("끌 항목이 서로 다른 폴더에 있음");
            return false;
        }
        let Some(folder) = Pidl::parse(parent) else {
            return false;
        };
        let items = self
            .paths
            .iter()
            .map(|path| Pidl::parse(path))
            .collect::<Option<Vec<_>>>();
        let Some(items) = items else {
            return false;
        };
        // 안전성: 각 PIDL은 items가 살아 있는 동안 유효하고, 마지막 ID는 그 안을 가리킴.
        let children = items
            .iter()
            .map(|item| unsafe { ILFindLastID(item.0) } as *const ITEMIDLIST)
            .collect::<Vec<_>>();
        let mut data: *mut c_void = std::ptr::null_mut();
        // 안전성: OLE 초기화는 같은 스레드에서 여러 번 불러도 됨(이미 초기화면 S_FALSE).
        // 데이터 객체는 끌기가 끝난 뒤 Release로 한 번 해제함.
        unsafe {
            OleInitialize(std::ptr::null());
            let result = SHCreateDataObject(
                folder.0,
                children.len() as u32,
                children.as_ptr(),
                std::ptr::null_mut(),
                &IID_IDATAOBJECT,
                &mut data,
            );
            if result < 0 || data.is_null() {
                zipelo_ui_memory_app::log_warn!("셸 데이터 객체를 만들지 못함: {result:#x}");
                return false;
            }
            let mut effect: DROPEFFECT = 0;
            let result = SHDoDragDrop(
                self.hwnd,
                data,
                std::ptr::null_mut(),
                DROPEFFECT_COPY,
                &mut effect,
            );
            release(data);
            zipelo_ui_memory_app::log_debug!("셸 끌기 끝: {result:#x}, 효과 {effect}");
            self.events.push_back(if result == DRAGDROP_S_DROP {
                DragEvent::Finished
            } else {
                DragEvent::Cancelled
            });
            result >= 0
        }
    }

    fn poll(&mut self) -> Option<DragEvent> {
        self.events.pop_front()
    }

    fn provide(&mut self, paths: &[PathBuf]) {
        self.paths = paths.to_vec();
    }

    fn fail(&mut self) {
        self.paths.clear();
    }
}

/// COM 객체의 IUnknown::Release를 부름.
unsafe fn release(object: *mut c_void) {
    #[repr(C)]
    struct Unknown {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
    }
    // 안전성: COM 객체의 첫 필드는 IUnknown으로 시작하는 vtable 포인터임.
    unsafe {
        let vtable = *(object as *const *const Unknown);
        ((*vtable).release)(object);
    }
}
