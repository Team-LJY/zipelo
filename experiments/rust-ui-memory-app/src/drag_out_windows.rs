//! Windows에서 압축파일 안 항목을 창 밖으로 끌어 놓기.
//!
//! 7-Zip 파일 관리자처럼 지연 제공(delayed rendering) 방식을 씀. 끌기는 바로 시작하고 앱은
//! 동시에 풀기를 시작함. 탐색기나 브라우저에 놓으면 받는 쪽이 CF_HDROP을 요청하는데,
//! 그때 다 풀릴 때까지 기다렸다가 풀린 파일 경로를 넘겨 받는 쪽이 놓은 위치로 복사함.
//!
//! OLE 끌기(SHDoDragDrop)는 놓거나 취소할 때까지 돌아오지 않으므로 별도 스레드에서
//! 진행함. 그동안 화면 스레드는 작업자의 풀기 진행을 계속 받음.

use std::{
    collections::VecDeque,
    ffi::c_void,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use windows_sys::{
    Win32::{
        Foundation::{DRAGDROP_S_DROP, HWND},
        System::{
            Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::{CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, OleInitialize, OleUninitialize},
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
            Shell::{SHCreateStdEnumFmtEtc, SHDoDragDrop},
            WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON},
        },
    },
    core::{GUID, HRESULT},
};
use zipelo_ui_memory_app::drag_out::{DragEvent, DragOut};

const S_OK: HRESULT = 0;
const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;
const E_NOTIMPL: HRESULT = 0x8000_4001_u32 as i32;
const E_FAIL: HRESULT = 0x8000_4005_u32 as i32;
const DV_E_FORMATETC: HRESULT = 0x8004_0064_u32 as i32;
const OLE_E_ADVISENOTSUPPORTED: HRESULT = 0x8004_0003_u32 as i32;
const DATA_S_SAMEFORMATETC: HRESULT = 0x0004_0130;
const TYMED_HGLOBAL: u32 = 1;
const DVASPECT_CONTENT: u32 = 1;
const DATADIR_GET: u32 = 1;

const IID_IUNKNOWN: GUID = GUID {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_IDATAOBJECT: GUID = GUID {
    data1: 0x0000_010e,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

/// FORMATETC와 같은 배치.
#[repr(C)]
#[derive(Clone, Copy)]
struct FormatEtc {
    format: u16,
    device: *mut c_void,
    aspect: u32,
    index: i32,
    tymed: u32,
}

/// STGMEDIUM과 같은 배치(공용체는 HGLOBAL 하나만 씀).
#[repr(C)]
struct StgMedium {
    tymed: u32,
    data: *mut c_void,
    release: *mut c_void,
}

/// DROPFILES와 같은 배치. 뒤에 UTF-16 경로 목록이 이어짐.
#[repr(C)]
struct DropFiles {
    files: u32,
    x: i32,
    y: i32,
    non_client: i32,
    wide: i32,
}

enum Ready {
    Paths(Vec<PathBuf>),
    Failed,
}

#[derive(Default)]
struct Shared {
    ready: Option<Ready>,
    events: VecDeque<DragEvent>,
    /// 끌기마다 늘림. 이전 끌기의 데이터 객체가 새 끌기의 상태를 쓰지 않게 함.
    generation: u64,
}

type State = Arc<(Mutex<Shared>, Condvar)>;

pub struct WindowsDrag {
    hwnd: isize,
    state: State,
    repaint: eframe::egui::Context,
}

/// 창이 Win32 창이면 끌어 놓기 구현을 만듦.
pub fn create(
    handles: &impl raw_window_handle::HasWindowHandle,
    repaint: eframe::egui::Context,
) -> Option<WindowsDrag> {
    let raw_window_handle::RawWindowHandle::Win32(window) = handles.window_handle().ok()?.as_raw()
    else {
        return None;
    };
    zipelo_ui_memory_app::log_debug!("Windows 끌어 놓기 준비");
    Some(WindowsDrag {
        hwnd: window.hwnd.get(),
        state: Arc::new((Mutex::new(Shared::default()), Condvar::new())),
        repaint,
    })
}

fn button_held() -> bool {
    // 안전성: 인자만 받는 Win32 상태 조회임.
    unsafe {
        let swapped = GetSystemMetrics(SM_SWAPBUTTON) != 0;
        let key = if swapped { VK_RBUTTON } else { VK_LBUTTON };
        GetAsyncKeyState(key as i32) < 0
    }
}

impl DragOut for WindowsDrag {
    fn start(&mut self) -> bool {
        if !button_held() {
            return false;
        }
        let generation = {
            let mut shared = self.state.0.lock().unwrap();
            shared.generation += 1;
            shared.ready = None;
            shared.events.clear();
            shared.generation
        };
        let state = self.state.clone();
        let hwnd = self.hwnd;
        let repaint = self.repaint.clone();
        std::thread::Builder::new()
            .name("zipelo-windows-drag".into())
            .spawn(move || {
                // 안전성: 이 스레드에서만 OLE를 초기화하고 끝날 때 해제함. 데이터 객체는
                // 끌기가 끝난 뒤 이 스레드가 가진 참조 하나를 놓음.
                let result = unsafe {
                    OleInitialize(std::ptr::null());
                    let object = DataObject::create(state.clone(), generation);
                    let mut effect: DROPEFFECT = 0;
                    let result = SHDoDragDrop(
                        hwnd as HWND,
                        object,
                        std::ptr::null_mut(),
                        DROPEFFECT_COPY,
                        &mut effect,
                    );
                    release(object);
                    OleUninitialize();
                    result
                };
                zipelo_ui_memory_app::log_debug!("셸 끌기 끝: {result:#x}");
                let mut shared = state.0.lock().unwrap();
                if shared.generation == generation {
                    shared.events.push_back(if result == DRAGDROP_S_DROP {
                        DragEvent::Finished
                    } else {
                        DragEvent::Cancelled
                    });
                }
                drop(shared);
                repaint.request_repaint();
            })
            .is_ok()
    }

    fn poll(&mut self) -> Option<DragEvent> {
        self.state.0.lock().unwrap().events.pop_front()
    }

    fn provide(&mut self, paths: &[PathBuf]) {
        self.state.0.lock().unwrap().ready = Some(Ready::Paths(paths.to_vec()));
        self.state.1.notify_all();
    }

    fn fail(&mut self) {
        self.state.0.lock().unwrap().ready = Some(Ready::Failed);
        self.state.1.notify_all();
    }
}

fn wide(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let text = path.as_os_str().encode_wide().collect::<Vec<_>>();
    // 셸은 \\?\ 접두사가 붙은 경로를 받지 않으므로 뗌.
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    if text.starts_with(&prefix) {
        text[prefix.len()..].to_vec()
    } else {
        text
    }
}

fn hdrop_format() -> FormatEtc {
    FormatEtc {
        format: CF_HDROP,
        device: std::ptr::null_mut(),
        aspect: DVASPECT_CONTENT,
        index: -1,
        tymed: TYMED_HGLOBAL,
    }
}

/// CF_HDROP만 제공하는 최소 IDataObject. 경로는 요청을 받을 때 풀기가 끝나기를 기다려 만듦.
#[repr(C)]
struct DataObject {
    vtable: *const DataObjectVtbl,
    refs: AtomicU32,
    state: State,
    generation: u64,
}

#[repr(C)]
struct DataObjectVtbl {
    query_interface:
        unsafe extern "system" fn(*mut DataObject, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut DataObject) -> u32,
    release: unsafe extern "system" fn(*mut DataObject) -> u32,
    get_data:
        unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> HRESULT,
    get_data_here:
        unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> HRESULT,
    query_get_data: unsafe extern "system" fn(*mut DataObject, *const FormatEtc) -> HRESULT,
    get_canonical_format_etc:
        unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut FormatEtc) -> HRESULT,
    set_data: unsafe extern "system" fn(
        *mut DataObject,
        *const FormatEtc,
        *const StgMedium,
        i32,
    ) -> HRESULT,
    enum_format_etc: unsafe extern "system" fn(*mut DataObject, u32, *mut *mut c_void) -> HRESULT,
    d_advise: unsafe extern "system" fn(
        *mut DataObject,
        *const FormatEtc,
        u32,
        *mut c_void,
        *mut u32,
    ) -> HRESULT,
    d_unadvise: unsafe extern "system" fn(*mut DataObject, u32) -> HRESULT,
    enum_d_advise: unsafe extern "system" fn(*mut DataObject, *mut *mut c_void) -> HRESULT,
}

static VTABLE: DataObjectVtbl = DataObjectVtbl {
    query_interface,
    add_ref,
    release: release_object,
    get_data,
    get_data_here,
    query_get_data,
    get_canonical_format_etc,
    set_data,
    enum_format_etc,
    d_advise,
    d_unadvise,
    enum_d_advise,
};

impl DataObject {
    /// 참조 하나를 가진 새 객체를 힙에 만듦.
    fn create(state: State, generation: u64) -> *mut c_void {
        Box::into_raw(Box::new(Self {
            vtable: &VTABLE,
            refs: AtomicU32::new(1),
            state,
            generation,
        }))
        .cast()
    }

    /// 다 풀릴 때까지 기다린 뒤 경로를 돌려줌. 실패했거나 새 끌기가 시작되면 None.
    fn wait_paths(&self) -> Option<Vec<PathBuf>> {
        let (lock, ready) = &*self.state;
        let mut shared = lock.lock().unwrap();
        loop {
            if shared.generation != self.generation {
                return None;
            }
            match &shared.ready {
                Some(Ready::Paths(paths)) => return Some(paths.clone()),
                Some(Ready::Failed) => return None,
                None => {
                    shared = ready
                        .wait_timeout(shared, Duration::from_millis(200))
                        .unwrap()
                        .0;
                }
            }
        }
    }
}

fn same_guid(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

fn supported(format: *const FormatEtc) -> bool {
    // 안전성: OLE가 넘기는 FORMATETC 포인터는 호출 동안 유효함.
    !format.is_null()
        && unsafe { (*format).format == CF_HDROP && (*format).tymed & TYMED_HGLOBAL != 0 }
}

unsafe extern "system" fn query_interface(
    this: *mut DataObject,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_NOINTERFACE;
    }
    // 안전성: OLE 규약대로 iid와 out은 호출 동안 유효함.
    unsafe {
        if !iid.is_null() && (same_guid(&*iid, &IID_IUNKNOWN) || same_guid(&*iid, &IID_IDATAOBJECT))
        {
            add_ref(this);
            *out = this.cast();
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn add_ref(this: *mut DataObject) -> u32 {
    // 안전성: 참조가 남아 있는 동안 객체는 살아 있음.
    unsafe { (*this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
}

unsafe extern "system" fn release_object(this: *mut DataObject) -> u32 {
    // 안전성: 마지막 참조를 놓을 때 한 번만 Box로 되돌려 해제함.
    unsafe {
        let left = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if left == 0 {
            drop(Box::from_raw(this));
        }
        left
    }
}

unsafe extern "system" fn get_data(
    this: *mut DataObject,
    format: *const FormatEtc,
    medium: *mut StgMedium,
) -> HRESULT {
    if !supported(format) || medium.is_null() {
        return DV_E_FORMATETC;
    }
    zipelo_ui_memory_app::log_debug!("받는 쪽이 파일 목록을 요청함. 다 풀릴 때까지 기다림");
    // 안전성: this는 참조가 남은 객체이고 medium은 호출 동안 유효함.
    let Some(paths) = (unsafe { (*this).wait_paths() }) else {
        return E_FAIL;
    };
    let mut list: Vec<u16> = Vec::new();
    for path in &paths {
        list.extend(wide(path));
        list.push(0);
    }
    list.push(0);
    let header = std::mem::size_of::<DropFiles>();
    let size = header + list.len() * 2;
    // 안전성: 할당한 메모리를 잠근 동안에만 쓰고, 받는 쪽이 STGMEDIUM으로 해제함.
    unsafe {
        let memory = GlobalAlloc(GHND, size);
        if memory.is_null() {
            return E_FAIL;
        }
        let base = GlobalLock(memory).cast::<u8>();
        if base.is_null() {
            return E_FAIL;
        }
        base.cast::<DropFiles>().write_unaligned(DropFiles {
            files: header as u32,
            x: 0,
            y: 0,
            non_client: 0,
            wide: 1,
        });
        std::ptr::copy_nonoverlapping(list.as_ptr().cast::<u8>(), base.add(header), list.len() * 2);
        GlobalUnlock(memory);
        *medium = StgMedium {
            tymed: TYMED_HGLOBAL,
            data: memory,
            release: std::ptr::null_mut(),
        };
    }
    S_OK
}

unsafe extern "system" fn get_data_here(
    _: *mut DataObject,
    _: *const FormatEtc,
    _: *mut StgMedium,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn query_get_data(_: *mut DataObject, format: *const FormatEtc) -> HRESULT {
    if supported(format) {
        S_OK
    } else {
        DV_E_FORMATETC
    }
}

unsafe extern "system" fn get_canonical_format_etc(
    _: *mut DataObject,
    _: *const FormatEtc,
    out: *mut FormatEtc,
) -> HRESULT {
    if !out.is_null() {
        // 안전성: out은 호출 동안 유효한 FORMATETC임.
        unsafe { (*out).device = std::ptr::null_mut() };
    }
    DATA_S_SAMEFORMATETC
}

unsafe extern "system" fn set_data(
    _: *mut DataObject,
    _: *const FormatEtc,
    _: *const StgMedium,
    _: i32,
) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn enum_format_etc(
    _: *mut DataObject,
    direction: u32,
    out: *mut *mut c_void,
) -> HRESULT {
    if direction != DATADIR_GET || out.is_null() {
        return E_NOTIMPL;
    }
    let format = hdrop_format();
    // 안전성: 셸이 FORMATETC 배열을 복사해 새 열거자를 만듦.
    unsafe { SHCreateStdEnumFmtEtc(1, (&format as *const FormatEtc).cast(), out) }
}

unsafe extern "system" fn d_advise(
    _: *mut DataObject,
    _: *const FormatEtc,
    _: u32,
    _: *mut c_void,
    _: *mut u32,
) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn d_unadvise(_: *mut DataObject, _: u32) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn enum_d_advise(_: *mut DataObject, _: *mut *mut c_void) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

/// COM 객체의 IUnknown::Release를 부름.
unsafe fn release(object: *mut c_void) {
    // 안전성: object는 이 모듈이 만든 DataObject임.
    unsafe {
        release_object(object.cast());
    }
}
