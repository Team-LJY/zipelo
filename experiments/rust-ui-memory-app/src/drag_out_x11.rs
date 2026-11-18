//! X11에서 압축파일 안 항목을 창 밖으로 끌어 놓기 (XDND 버전 5 끌기 쪽).
//!
//! winit은 끌어 놓기를 받기만 하므로 별도 X 연결에서 직접 끌기를 진행함. 버튼을 누르면
//! X 서버가 winit 연결에 포인터를 잡아 주므로(implicit grab), winit의 Xlib 연결에서
//! 그 잡기를 풀고 이 연결의 작은 창으로 다시 잡음. 받는 쪽이 XdndSelection을 요청하면
//! 풀기가 끝난 뒤 text/uri-list(풀린 파일 경로)로 답함.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            AtomEnum, ButtonReleaseEvent, ClientMessageEvent, ConnectionExt as _, CreateWindowAux,
            EventMask, GrabMode, GrabStatus, KeyButMask, MotionNotifyEvent, PropMode,
            SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};
use zipelo_ui_memory_app::drag_out::{DragEvent, DragOut, uri_list};

const XDND_VERSION: u32 = 5;
/// 놓기와 파일 준비가 모두 끝난 뒤에도 늦은 요청을 받을 수 있도록 기다리는 시간.
const READY_GRACE: Duration = Duration::from_secs(60);

/// 끌기 스레드를 끝내도 되는지 판단함.
///
/// 끌기를 취소하거나 받는 쪽이 없으면 바로 끝냄. 놓은 뒤에는 파일 준비가 끝나거나
/// 실패할 때까지 기다림. 두 조건이 충족된 뒤에도 늦은 요청을 위해 끌기 창을 남겨 둠.
fn session_over(
    grabbing: bool,
    dropped: bool,
    files_resolved: impl FnOnce() -> bool,
    grace_started_at: &mut Option<Instant>,
    waiting_requests: bool,
    now: Instant,
) -> bool {
    if grabbing {
        return false;
    }
    if !dropped {
        return true;
    }
    if grace_started_at.is_none() && files_resolved() {
        *grace_started_at = Some(now);
    }
    grace_started_at.is_some_and(|at| !waiting_requests && now.duration_since(at) >= READY_GRACE)
}

enum Ready {
    Paths(Vec<PathBuf>),
    Failed,
}

#[derive(Default)]
struct Shared {
    ready: Option<Ready>,
    events: VecDeque<DragEvent>,
    /// 끌기마다 늘림. 이전 끌기 스레드가 새 끌기의 상태를 건드리지 않게 함.
    generation: u64,
}

pub struct X11Drag {
    xlib: x11_dl::xlib::Xlib,
    /// winit이 쓰는 Xlib Display. 창이 살아 있는 동안 유효함.
    display: usize,
    /// winit 창. 끌기 대상에서 이 창(자기 자신)은 뺌.
    own: u32,
    shared: Arc<Mutex<Shared>>,
    repaint: eframe::egui::Context,
}

/// 창이 Xlib(X11) 창이면 끌어 놓기 구현을 만듦. 아니면 None.
pub fn create(
    handles: &(impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle),
    repaint: eframe::egui::Context,
) -> Option<X11Drag> {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    let RawDisplayHandle::Xlib(display) = handles.display_handle().ok()?.as_raw() else {
        return None;
    };
    let RawWindowHandle::Xlib(window) = handles.window_handle().ok()?.as_raw() else {
        return None;
    };
    let xlib = x11_dl::xlib::Xlib::open().ok()?;
    zipelo_ui_memory_app::log_debug!("X11 끌어 놓기 준비 (창 {:#x})", window.window);
    Some(X11Drag {
        xlib,
        display: display.display?.as_ptr() as usize,
        own: window.window as u32,
        shared: Arc::new(Mutex::new(Shared::default())),
        repaint,
    })
}

struct Atoms {
    aware: u32,
    proxy: u32,
    selection: u32,
    enter: u32,
    position: u32,
    status: u32,
    leave: u32,
    drop: u32,
    finished: u32,
    copy: u32,
    uri_list: u32,
    targets: u32,
}

impl Atoms {
    fn new(conn: &RustConnection) -> Option<Self> {
        let get = |name: &str| -> Option<u32> {
            Some(
                conn.intern_atom(false, name.as_bytes())
                    .ok()?
                    .reply()
                    .ok()?
                    .atom,
            )
        };
        Some(Self {
            aware: get("XdndAware")?,
            proxy: get("XdndProxy")?,
            selection: get("XdndSelection")?,
            enter: get("XdndEnter")?,
            position: get("XdndPosition")?,
            status: get("XdndStatus")?,
            leave: get("XdndLeave")?,
            drop: get("XdndDrop")?,
            finished: get("XdndFinished")?,
            copy: get("XdndActionCopy")?,
            uri_list: get("text/uri-list")?,
            targets: get("TARGETS")?,
        })
    }
}

impl DragOut for X11Drag {
    fn start(&mut self, _: &[PathBuf], _: &mut zipelo_ui_memory_app::drag_out::Pump<'_>) -> bool {
        match self.begin() {
            Ok(()) => true,
            Err(error) => {
                zipelo_ui_memory_app::log_warn!("X11 끌기를 시작하지 못함: {error}");
                false
            }
        }
    }

    fn poll(&mut self) -> Option<DragEvent> {
        self.shared.lock().unwrap().events.pop_front()
    }

    fn provide(&mut self, paths: &[PathBuf]) {
        self.shared.lock().unwrap().ready = Some(Ready::Paths(paths.to_vec()));
    }

    fn fail(&mut self) {
        self.shared.lock().unwrap().ready = Some(Ready::Failed);
    }
}

impl X11Drag {
    fn begin(&mut self) -> Result<(), String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn).ok_or("atom")?;
        let pointer = conn
            .query_pointer(root)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if !pointer.mask.contains(KeyButMask::BUTTON1) {
            return Err("왼쪽 버튼을 누르고 있지 않음".into());
        }
        // 화면 밖의 1x1 입력 전용 창. 포인터를 잡고 XdndSelection을 가짐.
        let window = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_window(
            0,
            window,
            root,
            -10,
            -10,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().override_redirect(1),
        )
        .map_err(|e| e.to_string())?;
        conn.map_window(window).map_err(|e| e.to_string())?;
        conn.set_selection_owner(window, atoms.selection, CURRENT_TIME)
            .map_err(|e| e.to_string())?;
        let cursor = drag_cursor(&conn).unwrap_or(NONE);
        conn.flush().map_err(|e| e.to_string())?;
        // winit 연결이 가진 버튼 누름 잡기를 풀어야 다른 연결이 포인터를 잡을 수 있음.
        // 안전성: display는 winit이 창과 함께 유지하는 Xlib Display이고 같은 스레드에서 씀.
        unsafe {
            (self.xlib.XUngrabPointer)(self.display as *mut _, 0);
            (self.xlib.XFlush)(self.display as *mut _);
        }
        let mut grabbed = false;
        for _ in 0..20 {
            let status = conn
                .grab_pointer(
                    false,
                    window,
                    EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    NONE,
                    cursor,
                    CURRENT_TIME,
                )
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .status;
            if status == GrabStatus::SUCCESS {
                grabbed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !grabbed {
            let _ = conn.destroy_window(window);
            let _ = conn.flush();
            return Err("포인터를 잡지 못함".into());
        }
        let generation = {
            let mut shared = self.shared.lock().unwrap();
            shared.generation += 1;
            shared.ready = None;
            shared.events.clear();
            shared.generation
        };
        let session = Session {
            conn,
            root,
            window,
            own: self.own,
            atoms,
            shared: self.shared.clone(),
            generation,
            repaint: self.repaint.clone(),
            target: None,
            accepted: false,
            waiting_status: false,
            last_position: None,
            requests: Vec::new(),
            done: false,
        };
        std::thread::Builder::new()
            .name("zipelo-x11-drag".into())
            .spawn(move || session.run())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// X 커서 글꼴의 손 모양(XC_hand2) 커서.
fn drag_cursor(conn: &RustConnection) -> Option<u32> {
    let font = conn.generate_id().ok()?;
    conn.open_font(font, b"cursor").ok()?;
    let cursor = conn.generate_id().ok()?;
    conn.create_glyph_cursor(cursor, font, font, 60, 61, 0, 0, 0, 0xffff, 0xffff, 0xffff)
        .ok()?;
    conn.close_font(font).ok()?;
    Some(cursor)
}

#[derive(Clone, Copy, PartialEq)]
struct Target {
    window: u32,
    /// XdndProxy가 있으면 메시지를 보낼 창. 없으면 window와 같음.
    send_to: u32,
    version: u32,
}

struct Session {
    conn: RustConnection,
    root: u32,
    window: u32,
    own: u32,
    atoms: Atoms,
    shared: Arc<Mutex<Shared>>,
    generation: u64,
    repaint: eframe::egui::Context,
    target: Option<Target>,
    accepted: bool,
    waiting_status: bool,
    last_position: Option<(i16, i16, u32)>,
    /// 파일이 준비되기 전에 들어온 선택 요청.
    requests: Vec<SelectionRequestEvent>,
    /// 받는 쪽이 XdndFinished를 보냄.
    done: bool,
}

impl Session {
    fn run(mut self) {
        let mut dropped = false;
        let mut grace_started_at = None;
        let mut grabbing = true;
        loop {
            self.answer_waiting();
            let event = match self.conn.poll_for_event() {
                Ok(event) => event,
                Err(error) => {
                    zipelo_ui_memory_app::log_warn!("X11 끌기 연결 오류: {error}");
                    self.push(DragEvent::Cancelled);
                    break;
                }
            };
            let Some(event) = event else {
                if session_over(
                    grabbing,
                    dropped,
                    || self.shared.lock().unwrap().ready.is_some(),
                    &mut grace_started_at,
                    !self.requests.is_empty(),
                    Instant::now(),
                ) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            match event {
                Event::MotionNotify(MotionNotifyEvent {
                    root_x,
                    root_y,
                    time,
                    ..
                }) if grabbing => self.motion(root_x, root_y, time),
                Event::ButtonRelease(ButtonReleaseEvent { time, .. }) if grabbing => {
                    grabbing = false;
                    let _ = self.conn.ungrab_pointer(CURRENT_TIME);
                    match self.target {
                        Some(target) if self.accepted => {
                            self.message(target, self.atoms.drop, [0, time, 0]);
                            self.push(DragEvent::Dropped);
                            dropped = true;
                        }
                        Some(target) => {
                            self.message(target, self.atoms.leave, [0, 0, 0]);
                            self.push(DragEvent::Cancelled);
                        }
                        None => self.push(DragEvent::Cancelled),
                    }
                    let _ = self.conn.flush();
                }
                Event::ClientMessage(message) => self.client_message(&message),
                Event::SelectionRequest(request) => self.selection_request(request),
                _ => {}
            }
            if self.finished() {
                break;
            }
        }
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
    }

    fn finished(&self) -> bool {
        self.done || self.shared.lock().unwrap().generation != self.generation
    }

    fn push(&self, event: DragEvent) {
        let mut shared = self.shared.lock().unwrap();
        if shared.generation == self.generation {
            shared.events.push_back(event);
        }
        drop(shared);
        self.repaint.request_repaint();
    }

    fn message(&self, target: Target, kind: u32, rest: [u32; 3]) {
        let data = [self.window, rest[0], rest[1], rest[2], 0];
        let data = if kind == self.atoms.position {
            [self.window, 0, rest[1], rest[2], self.atoms.copy]
        } else {
            data
        };
        let event = ClientMessageEvent::new(32, target.window, kind, data);
        let _ = self
            .conn
            .send_event(false, target.send_to, EventMask::NO_EVENT, event);
    }

    fn motion(&mut self, x: i16, y: i16, time: u32) {
        let target = self.find_target(x, y);
        if target != self.target {
            if let Some(old) = self.target {
                self.message(old, self.atoms.leave, [0, 0, 0]);
            }
            self.accepted = false;
            self.waiting_status = false;
            if let Some(new) = target {
                let data = [
                    self.window,
                    new.version.min(XDND_VERSION) << 24,
                    self.atoms.uri_list,
                    0,
                    0,
                ];
                let event = ClientMessageEvent::new(32, new.window, self.atoms.enter, data);
                let _ = self
                    .conn
                    .send_event(false, new.send_to, EventMask::NO_EVENT, event);
            }
            self.target = target;
        }
        self.last_position = Some((x, y, time));
        if !self.waiting_status {
            self.send_position();
        }
        let _ = self.conn.flush();
    }

    fn send_position(&mut self) {
        let (Some(target), Some((x, y, time))) = (self.target, self.last_position.take()) else {
            return;
        };
        let point = ((x as u16 as u32) << 16) | (y as u16 as u32);
        self.message(target, self.atoms.position, [0, point, time]);
        self.waiting_status = true;
    }

    fn client_message(&mut self, message: &ClientMessageEvent) {
        let data = message.data.as_data32();
        if message.type_ == self.atoms.status
            && self.target.is_some_and(|target| target.window == data[0])
        {
            self.accepted = data[1] & 1 == 1;
            self.waiting_status = false;
            self.send_position();
            let _ = self.conn.flush();
        } else if message.type_ == self.atoms.finished {
            self.done = true;
            self.push(DragEvent::Finished);
        }
    }

    /// 포인터 아래에서 XdndAware를 가진 창을 찾음. Zipelo 자신의 창은 대상으로 하지 않음.
    fn find_target(&self, x: i16, y: i16) -> Option<Target> {
        let mut parent = self.root;
        for _ in 0..32 {
            let child = self
                .conn
                .translate_coordinates(self.root, parent, x, y)
                .ok()?
                .reply()
                .ok()?
                .child;
            if child == NONE || child == self.own || child == self.window {
                return None;
            }
            if let Some(target) = self.aware(child) {
                return (target.window != self.own).then_some(target);
            }
            parent = child;
        }
        None
    }

    fn aware(&self, window: u32) -> Option<Target> {
        let proxy = self
            .conn
            .get_property(false, window, self.atoms.proxy, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()
            .and_then(|reply| reply.value32()?.next())
            .unwrap_or(window);
        let version = self
            .conn
            .get_property(false, proxy, self.atoms.aware, AtomEnum::ATOM, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()?;
        (version >= 3).then_some(Target {
            window,
            send_to: proxy,
            version,
        })
    }

    fn selection_request(&mut self, request: SelectionRequestEvent) {
        if request.selection != self.atoms.selection {
            self.notify(&request, false);
            return;
        }
        if request.target == self.atoms.targets {
            let property = property_of(&request);
            let _ = self.conn.change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::ATOM,
                &[self.atoms.targets, self.atoms.uri_list],
            );
            self.notify(&request, true);
        } else if request.target == self.atoms.uri_list {
            self.requests.push(request);
            self.answer_waiting();
        } else {
            self.notify(&request, false);
        }
    }

    /// 파일이 준비됐으면 기다리던 요청에 답함.
    fn answer_waiting(&mut self) {
        if self.requests.is_empty() {
            return;
        }
        let shared = self.shared.lock().unwrap();
        let data = match &shared.ready {
            None => return,
            Some(Ready::Failed) => None,
            Some(Ready::Paths(paths)) => Some(uri_list(paths)),
        };
        drop(shared);
        for request in std::mem::take(&mut self.requests) {
            match &data {
                Some(text) => {
                    let _ = self.conn.change_property8(
                        PropMode::REPLACE,
                        request.requestor,
                        property_of(&request),
                        self.atoms.uri_list,
                        text.as_bytes(),
                    );
                    self.notify(&request, true);
                }
                None => self.notify(&request, false),
            }
        }
        let _ = self.conn.flush();
    }

    fn notify(&self, request: &SelectionRequestEvent, ok: bool) {
        let event = SelectionNotifyEvent {
            response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if ok { property_of(request) } else { NONE },
        };
        let _ = self
            .conn
            .send_event(false, request.requestor, EventMask::NO_EVENT, event);
        let _ = self.conn.flush();
    }
}

/// 오래된 클라이언트는 property를 비워 보내므로 target을 property로 씀.
fn property_of(request: &SelectionRequestEvent) -> u32 {
    if request.property == NONE {
        request.target
    } else {
        request.property
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-app-ui/drag_out_x11_lifetime.rs"]
mod tests;
