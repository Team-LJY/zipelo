//! Windows에서 압축파일 안 항목을 창 밖으로 끌어 놓기.
//!
//! 7-Zip 파일 관리자와 같은 흐름을 씀. 끌기를 시작하면 앱은 바로 풀기를 시작하고, OLE 끌기
//! (DoDragDrop)는 버튼을 누른 창의 스레드(화면 스레드)에서 진행함(다른 스레드에서는 마우스
//! 입력을 받지 못함). 끌기는 놓거나 취소할 때까지 돌아오지 않으므로 그동안 앱이 넘긴 pump로
//! 작업자의 풀기 진행을 받음.
//!
//! - 끌고 다니는 동안: 받는 쪽(Zipelo 창 자신 포함)이 목록을 물으면 기다리지 않고 풀릴 예정인
//!   경로를 돌려줌. 그래서 끌기와 화면이 멈추지 않음. Esc를 누르면 그 자리에서 풀기를 취소함.
//! - 버튼을 놓았을 때: 이미 다 풀렸으면 바로 놓음. 아직이면 Windows 기본 진행률 창(취소 가능)을
//!   띄우고 다 풀린 뒤에 놓기를 완료해, 받는 쪽은 놓은 위치로 완성된 파일을 복사함.

use std::{
    cell::Cell,
    collections::VecDeque,
    ffi::c_void,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::{
            DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, POINT,
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::{CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DoDragDrop, OleInitialize},
            Threading::GetCurrentProcessId,
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON, VK_RBUTTON},
            Shell::SHCreateStdEnumFmtEtc,
            WindowsAndMessaging::{
                DispatchMessageW, GA_ROOT, GetAncestor, GetCursorPos, GetSystemMetrics,
                GetWindowThreadProcessId, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW,
                PostQuitMessage, QS_ALLINPUT, SM_SWAPBUTTON, TranslateMessage, WM_KEYDOWN, WM_QUIT,
                WindowFromPoint,
            },
        },
    },
    core::{GUID, HRESULT},
};
use zipelo_ui_memory_app::drag_out::{
    DragEvent, DragOut, Pump, PumpState, ReleaseAction, is_own_window, release_action,
};

thread_local! {
    /// 끌기가 진행되는 동안(start 안에서만) 앱이 넘긴 작업 진행 함수.
    static PUMP: Cell<Option<*mut Pump<'static>>> = const { Cell::new(None) };
}

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
const MK_LBUTTON: u32 = 0x1;
const MK_RBUTTON: u32 = 0x2;
/// 놓은 뒤 이 시간 안에 다 풀리면 진행률 창을 띄우지 않음.
const PROGRESS_DELAY: Duration = Duration::from_millis(300);

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> GUID {
    GUID {
        data1,
        data2,
        data3,
        data4,
    }
}
const OLE_TAIL: [u8; 8] = [0xC0, 0, 0, 0, 0, 0, 0, 0x46];
const IID_IUNKNOWN: GUID = guid(0, 0, 0, OLE_TAIL);
const IID_IDATAOBJECT: GUID = guid(0x0000_010e, 0, 0, OLE_TAIL);
const IID_IDROPSOURCE: GUID = guid(0x0000_0121, 0, 0, OLE_TAIL);
const CLSID_PROGRESS_DIALOG: GUID = guid(
    0xF838_3852,
    0xFCD3,
    0x11d1,
    [0xA6, 0xB9, 0x00, 0x60, 0x97, 0xDF, 0x5B, 0xD4],
);
const IID_IPROGRESS_DIALOG: GUID = guid(
    0xEBBC_7C04,
    0x315E,
    0x11d2,
    [0xB6, 0x2F, 0x00, 0x60, 0x97, 0xDF, 0x5B, 0xD4],
);

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

#[derive(Default)]
struct Shared {
    /// 다 풀린 뒤 넘길 경로. 놓기 전에 목록을 물으면 이 경로를 돌려줌.
    planned: Vec<PathBuf>,
    /// 다 풀렸으면 Some(true), 실패했으면 Some(false).
    ready: Option<bool>,
    events: VecDeque<DragEvent>,
}

type State = Arc<Mutex<Shared>>;

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
    // 안전성: 화면 스레드에서 OLE를 초기화함. winit이 이미 했으면 S_FALSE이며 앱이 끝날
    // 때까지 유지함.
    let result = unsafe { OleInitialize(std::ptr::null()) };
    zipelo_ui_memory_app::log_debug!("Windows 끌어 놓기 준비 (OLE {result:#x})");
    Some(WindowsDrag {
        hwnd: window.hwnd.get(),
        state: Arc::new(Mutex::new(Shared::default())),
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

/// Esc가 지금 눌려 있거나 지난 확인 이후 눌린 적이 있는지. 짧게 누른 것도 놓치지 않음.
fn escape_pressed() -> bool {
    // 안전성: 인자만 받는 Win32 상태 조회임.
    let state = unsafe { GetAsyncKeyState(VK_ESCAPE as i32) };
    state < 0 || state & 1 != 0
}

/// 화면 스레드에 쌓인 메시지를 처리함. 기다리는 동안에도 진행률 창이 표시되고 Zipelo 창이
/// 응답 없음으로 바뀌지 않게 함. 창에 온 Esc 키를 보았거나 종료 요청이 오면 true.
fn pump_messages() -> bool {
    let mut cancel = false;
    // 안전성: 표준 메시지 루프임. 받은 메시지는 같은 스레드에서 바로 처리함.
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            if message.message == WM_QUIT {
                // 종료 요청은 끌기가 끝난 뒤 앱 메시지 루프가 받도록 다시 넣고 끌기를 취소함.
                PostQuitMessage(message.wParam as i32);
                return true;
            }
            if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize {
                cancel = true;
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    cancel
}

/// 커서 아래 최상위 창이 Zipelo 프로세스의 창인지.
fn cursor_over_own_window() -> bool {
    // 안전성: 인자만 받는 Win32 상태 조회이며 POINT는 호출 동안 유효함.
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            return false;
        }
        let window = WindowFromPoint(point);
        if window.is_null() {
            return false;
        }
        let root = GetAncestor(window, GA_ROOT);
        let top = if root.is_null() { window } else { root };
        let mut process = 0u32;
        GetWindowThreadProcessId(top, &mut process);
        is_own_window((process != 0).then_some(process), GetCurrentProcessId())
    }
}

/// 메시지가 오거나 시간이 지날 때까지 기다림.
fn wait_messages(timeout: Duration) {
    // 안전성: 기다릴 핸들 없이 메시지 도착만 기다림.
    unsafe {
        MsgWaitForMultipleObjects(
            0,
            std::ptr::null(),
            0,
            timeout.as_millis() as u32,
            QS_ALLINPUT,
        );
    }
}

/// 끌기 중에 앱의 작업 진행을 받음. start 밖에서는 실패로 봄.
fn pump(cancel: bool) -> PumpState {
    match PUMP.with(Cell::get) {
        // 안전성: PUMP는 start가 진행 중인 같은 스레드에서만 설정돼 있음.
        Some(pump) => unsafe { (*pump)(cancel) },
        None => PumpState::Failed,
    }
}

impl DragOut for WindowsDrag {
    fn start(&mut self, planned: &[PathBuf], pump: &mut Pump<'_>) -> bool {
        if !button_held() {
            return false;
        }
        {
            let mut shared = self.state.lock().unwrap();
            shared.planned = planned.to_vec();
            shared.ready = None;
            shared.events.clear();
        }
        // 안전성: pump는 이 함수가 돌아오기 전까지만 쓰고, 끝나면 바로 지움. 데이터 객체와
        // 끌기 원본의 메서드는 DoDragDrop 안에서 같은 스레드로 불림.
        let pump: *mut Pump<'_> = pump;
        PUMP.with(|cell| {
            cell.set(Some(unsafe {
                std::mem::transmute::<*mut Pump<'_>, *mut Pump<'static>>(pump)
            }))
        });
        // 안전성: 두 객체는 끌기가 끝난 뒤 이 함수가 가진 참조 하나씩을 놓음.
        let result = unsafe {
            let data = DataObject::create(self.state.clone());
            let source = DropSource::create(self.state.clone(), self.hwnd);
            let mut effect: DROPEFFECT = 0;
            let result = DoDragDrop(data, source, DROPEFFECT_COPY, &mut effect);
            release_unknown(source);
            release_unknown(data);
            result
        };
        PUMP.with(|cell| cell.set(None));
        zipelo_ui_memory_app::log_debug!("셸 끌기 끝: {result:#x}");
        self.state
            .lock()
            .unwrap()
            .events
            .push_back(if result == DRAGDROP_S_DROP {
                DragEvent::Finished
            } else {
                DragEvent::Cancelled
            });
        self.repaint.request_repaint();
        true
    }

    fn poll(&mut self) -> Option<DragEvent> {
        self.state.lock().unwrap().events.pop_front()
    }

    fn provide(&mut self, paths: &[PathBuf]) {
        let mut shared = self.state.lock().unwrap();
        shared.planned = paths.to_vec();
        shared.ready = Some(true);
    }

    fn fail(&mut self) {
        self.state.lock().unwrap().ready = Some(false);
    }
}

fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
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

fn same_guid(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// IUnknown 부분을 공유하는 COM 객체 머리. 두 객체 모두 첫 필드가 vtable 포인터임.
#[repr(C)]
struct UnknownVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// COM 객체의 IUnknown::Release를 부름.
unsafe fn release_unknown(object: *mut c_void) {
    // 안전성: object는 IUnknown으로 시작하는 vtable을 가진 COM 객체임.
    unsafe {
        let vtable = *(object as *const *const UnknownVtbl);
        ((*vtable).release)(object);
    }
}

// ---- 데이터 객체 -------------------------------------------------------------------------

/// CF_HDROP만 제공하는 최소 IDataObject. 어떤 요청에도 기다리지 않음.
#[repr(C)]
struct DataObject {
    vtable: *const DataObjectVtbl,
    refs: AtomicU32,
    state: State,
}

#[repr(C)]
struct DataObjectVtbl {
    unknown: UnknownVtbl,
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

static DATA_VTABLE: DataObjectVtbl = DataObjectVtbl {
    unknown: UnknownVtbl {
        query_interface: data_query_interface,
        add_ref: data_add_ref,
        release: data_release,
    },
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
    fn create(state: State) -> *mut c_void {
        Box::into_raw(Box::new(Self {
            vtable: &DATA_VTABLE,
            refs: AtomicU32::new(1),
            state,
        }))
        .cast()
    }
}

unsafe extern "system" fn data_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // 안전성: OLE 규약대로 iid와 out은 호출 동안 유효함.
    unsafe { query(this, iid, out, &IID_IDATAOBJECT, data_add_ref) }
}

unsafe extern "system" fn data_add_ref(this: *mut c_void) -> u32 {
    // 안전성: 참조가 남아 있는 동안 객체는 살아 있음.
    unsafe {
        (*this.cast::<DataObject>())
            .refs
            .fetch_add(1, Ordering::AcqRel)
            + 1
    }
}

unsafe extern "system" fn data_release(this: *mut c_void) -> u32 {
    // 안전성: 마지막 참조를 놓을 때 한 번만 Box로 되돌려 해제함.
    unsafe {
        let object = this.cast::<DataObject>();
        let left = (*object).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if left == 0 {
            drop(Box::from_raw(object));
        }
        left
    }
}

/// IUnknown과 지정한 인터페이스만 돌려주는 QueryInterface 공통 처리.
unsafe fn query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
    own: &GUID,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
) -> HRESULT {
    if out.is_null() {
        return E_NOINTERFACE;
    }
    // 안전성: 호출한 쪽이 넘긴 포인터는 호출 동안 유효함.
    unsafe {
        if !iid.is_null() && (same_guid(&*iid, &IID_IUNKNOWN) || same_guid(&*iid, own)) {
            add_ref(this);
            *out = this;
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}

fn supported(format: *const FormatEtc) -> bool {
    // 안전성: OLE가 넘기는 FORMATETC 포인터는 호출 동안 유효함.
    !format.is_null()
        && unsafe { (*format).format == CF_HDROP && (*format).tymed & TYMED_HGLOBAL != 0 }
}

unsafe extern "system" fn get_data(
    this: *mut DataObject,
    format: *const FormatEtc,
    medium: *mut StgMedium,
) -> HRESULT {
    if !supported(format) || medium.is_null() {
        return DV_E_FORMATETC;
    }
    // 안전성: this는 참조가 남은 객체임.
    let (paths, ready) = unsafe {
        let shared = (*this).state.lock().unwrap();
        (shared.planned.clone(), shared.ready)
    };
    if ready == Some(false) || paths.is_empty() {
        return E_FAIL;
    }
    // 놓기 전이면 아직 풀리지 않은 예정 경로를 돌려줌. 놓을 때는 끌기 원본이 다 풀리기를
    // 기다린 뒤에 놓기를 완료하므로 받는 쪽은 완성된 파일을 받음.
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
    let format = FormatEtc {
        format: CF_HDROP,
        device: std::ptr::null_mut(),
        aspect: DVASPECT_CONTENT,
        index: -1,
        tymed: TYMED_HGLOBAL,
    };
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

// ---- 끌기 원본 ---------------------------------------------------------------------------

/// 끌기 중 버튼과 Esc를 보고 끌기를 계속할지 정하는 IDropSource.
#[repr(C)]
struct DropSource {
    vtable: *const DropSourceVtbl,
    refs: AtomicU32,
    state: State,
    hwnd: isize,
}

#[repr(C)]
struct DropSourceVtbl {
    unknown: UnknownVtbl,
    query_continue_drag: unsafe extern "system" fn(*mut DropSource, i32, u32) -> HRESULT,
    give_feedback: unsafe extern "system" fn(*mut DropSource, u32) -> HRESULT,
}

static SOURCE_VTABLE: DropSourceVtbl = DropSourceVtbl {
    unknown: UnknownVtbl {
        query_interface: source_query_interface,
        add_ref: source_add_ref,
        release: source_release,
    },
    query_continue_drag,
    give_feedback,
};

impl DropSource {
    fn create(state: State, hwnd: isize) -> *mut c_void {
        Box::into_raw(Box::new(Self {
            vtable: &SOURCE_VTABLE,
            refs: AtomicU32::new(1),
            state,
            hwnd,
        }))
        .cast()
    }

    fn set_ready(&self, ready: PumpState) {
        let mut shared = self.state.lock().unwrap();
        match ready {
            PumpState::Ready(paths) => {
                shared.planned = paths;
                shared.ready = Some(true);
            }
            PumpState::Failed => shared.ready = Some(false),
            PumpState::Waiting { .. } => {}
        }
    }

    fn ready(&self) -> Option<bool> {
        self.state.lock().unwrap().ready
    }

    /// 버튼을 놓았는데 아직 다 풀리지 않았으면 진행률 창을 띄우고 기다림.
    /// 다 풀리면 true, 취소했거나 실패하면 false.
    fn wait_for_files(&self) -> bool {
        let started = Instant::now();
        let mut dialog: Option<ProgressDialog> = None;
        // 기다리기 전에 눌렸던 Esc 기록은 지움.
        escape_pressed();
        loop {
            // 메시지를 처리해야 진행률 창(Zipelo 창이 부모)이 표시되고 Zipelo 창도 응답함.
            let cancelled =
                pump_messages() | escape_pressed() | dialog.as_ref().is_some_and(|d| d.cancelled());
            let state = pump(cancelled);
            match state {
                PumpState::Waiting { done, total } => {
                    if cancelled {
                        self.set_ready(PumpState::Failed);
                        return false;
                    }
                    if dialog.is_none() && started.elapsed() >= PROGRESS_DELAY {
                        dialog = ProgressDialog::open(self.hwnd);
                    }
                    if let Some(dialog) = &dialog {
                        dialog.progress(done, total);
                    }
                    wait_messages(Duration::from_millis(30));
                }
                ready => {
                    let ok = matches!(ready, PumpState::Ready(_));
                    self.set_ready(ready);
                    return ok;
                }
            }
        }
    }
}

unsafe extern "system" fn source_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // 안전성: OLE 규약대로 iid와 out은 호출 동안 유효함.
    unsafe { query(this, iid, out, &IID_IDROPSOURCE, source_add_ref) }
}

unsafe extern "system" fn source_add_ref(this: *mut c_void) -> u32 {
    // 안전성: 참조가 남아 있는 동안 객체는 살아 있음.
    unsafe {
        (*this.cast::<DropSource>())
            .refs
            .fetch_add(1, Ordering::AcqRel)
            + 1
    }
}

unsafe extern "system" fn source_release(this: *mut c_void) -> u32 {
    // 안전성: 마지막 참조를 놓을 때 한 번만 Box로 되돌려 해제함.
    unsafe {
        let object = this.cast::<DropSource>();
        let left = (*object).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if left == 0 {
            drop(Box::from_raw(object));
        }
        left
    }
}

unsafe extern "system" fn query_continue_drag(
    this: *mut DropSource,
    escape: i32,
    keys: u32,
) -> HRESULT {
    // 안전성: this는 DoDragDrop이 쓰는 동안 살아 있는 객체임.
    let source = unsafe { &*this };
    if escape != 0 {
        // 끌기를 취소하면 받을 곳이 없으므로 풀기도 바로 취소함.
        pump(true);
        source.set_ready(PumpState::Failed);
        return DRAGDROP_S_CANCEL;
    }
    // 끌고 다니는 동안에도 작업자 진행을 받음(막히지 않음).
    if source.ready().is_none() {
        let state = pump(false);
        if state == PumpState::Failed {
            // 암호 등 사용자 응답이 필요하거나 실패함. 끌기를 끝내고 앱 화면에서 안내함.
            source.set_ready(PumpState::Failed);
            return DRAGDROP_S_CANCEL;
        }
        source.set_ready(state);
    }
    if keys & (MK_LBUTTON | MK_RBUTTON) != 0 {
        return S_OK;
    }
    // 버튼을 놓음: Zipelo 창 위면 기다리지 않고 끌기와 풀기를 함께 취소하고, 아니면 다 풀린
    // 뒤에 놓기를 완료함.
    let over_own = cursor_over_own_window();
    match release_action(source.ready(), over_own) {
        ReleaseAction::Drop => DRAGDROP_S_DROP,
        ReleaseAction::Cancel => {
            if over_own {
                zipelo_ui_memory_app::log_debug!("Zipelo 창 위에 놓음: 끌기와 풀기를 취소함");
            }
            pump(true);
            source.set_ready(PumpState::Failed);
            DRAGDROP_S_CANCEL
        }
        ReleaseAction::Wait if source.wait_for_files() => DRAGDROP_S_DROP,
        ReleaseAction::Wait => DRAGDROP_S_CANCEL,
    }
}

unsafe extern "system" fn give_feedback(_: *mut DropSource, _: u32) -> HRESULT {
    DRAGDROP_S_USEDEFAULTCURSORS
}

// ---- 진행률 창 ---------------------------------------------------------------------------

/// Windows 기본 진행률 창(IProgressDialog). 자기 스레드에서 돌아 화면이 멈춰도 응답함.
struct ProgressDialog(*mut c_void);

#[repr(C)]
struct ProgressDialogVtbl {
    unknown: UnknownVtbl,
    start: unsafe extern "system" fn(*mut c_void, HWND, *mut c_void, u32, *const c_void) -> HRESULT,
    stop: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    set_animation: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> HRESULT,
    has_user_cancelled: unsafe extern "system" fn(*mut c_void) -> i32,
    set_progress: unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT,
    set_progress64: unsafe extern "system" fn(*mut c_void, u64, u64) -> HRESULT,
    set_line:
        unsafe extern "system" fn(*mut c_void, u32, *const u16, i32, *const c_void) -> HRESULT,
    set_cancel_msg: unsafe extern "system" fn(*mut c_void, *const u16, *const c_void) -> HRESULT,
}

/// PROGDLG_AUTOTIME | PROGDLG_NOMINIMIZE
const PROGRESS_FLAGS: u32 = 0x2 | 0x8;

impl ProgressDialog {
    fn vtable(&self) -> &ProgressDialogVtbl {
        // 안전성: self.0은 CoCreateInstance가 돌려준 IProgressDialog임.
        unsafe { &**(self.0 as *const *const ProgressDialogVtbl) }
    }

    fn open(hwnd: isize) -> Option<Self> {
        let mut object: *mut c_void = std::ptr::null_mut();
        // 안전성: 표준 셸 COM 객체를 만들고 실패하면 쓰지 않음.
        let result = unsafe {
            CoCreateInstance(
                &CLSID_PROGRESS_DIALOG,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IPROGRESS_DIALOG,
                &mut object,
            )
        };
        if result < 0 || object.is_null() {
            zipelo_ui_memory_app::log_warn!("진행률 창을 만들지 못함: {result:#x}");
            return None;
        }
        let dialog = Self(object);
        let title = wide_text("Zipelo");
        let line1 = wide_text("끌어 놓은 파일을 푸는 중입니다");
        let line2 = wide_text("다 풀리면 놓은 곳으로 옮깁니다");
        let cancel = wide_text("취소하는 중입니다");
        let vtable = dialog.vtable();
        // 안전성: 문자열은 각 호출 동안 살아 있고, 창은 Drop에서 닫음.
        unsafe {
            (vtable.set_title)(object, title.as_ptr());
            (vtable.set_line)(object, 1, line1.as_ptr(), 0, std::ptr::null());
            (vtable.set_line)(object, 2, line2.as_ptr(), 0, std::ptr::null());
            (vtable.set_cancel_msg)(object, cancel.as_ptr(), std::ptr::null());
            (vtable.start)(
                object,
                hwnd as HWND,
                std::ptr::null_mut(),
                PROGRESS_FLAGS,
                std::ptr::null(),
            );
        }
        Some(dialog)
    }

    fn progress(&self, done: u64, total: Option<u64>) {
        // 안전성: 열린 진행률 창에 값만 넘김.
        unsafe {
            (self.vtable().set_progress64)(self.0, done, total.unwrap_or(0).max(done));
        }
    }

    fn cancelled(&self) -> bool {
        // 안전성: 열린 진행률 창의 상태 조회임.
        unsafe { (self.vtable().has_user_cancelled)(self.0) != 0 }
    }
}

impl Drop for ProgressDialog {
    fn drop(&mut self) {
        // 안전성: 창을 닫고 참조를 한 번 놓음.
        unsafe {
            (self.vtable().stop)(self.0);
            release_unknown(self.0);
        }
    }
}
