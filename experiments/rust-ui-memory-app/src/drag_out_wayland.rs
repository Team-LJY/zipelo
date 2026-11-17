//! Wayland에서 압축파일 안 항목을 창 밖으로 끌어 놓기.
//!
//! winit은 끌어 놓기를 받기만 하고 시작하지는 못하므로, winit이 쓰는 같은 Wayland 연결에
//! 별도 이벤트 큐를 만들어 wl_data_device로 직접 끌기를 시작함. 끌기 시작에는 버튼을 누른
//! 순간의 serial이 필요하므로 같은 seat에서 포인터를 하나 더 받아 왼쪽 버튼 누름을 기억함.
//!
//! 받는 쪽에는 text/uri-list(풀린 파일 경로)와, 샌드박스 앱(스냅·Flatpak 브라우저 등)을
//! 위한 application/vnd.portal.filetransfer(문서 포털 전송 키)를 함께 제공함.

use std::{
    collections::VecDeque,
    io::Write,
    os::fd::OwnedFd,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child,
    protocol::{
        wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_pointer,
        wl_registry, wl_seat, wl_surface,
    },
};
use zipelo_ui_memory_app::drag_out::{DragEvent, DragOut, uri_list};

const URI_LIST: &str = "text/uri-list";
const PORTAL: &str = "application/vnd.portal.filetransfer";
/// linux/input-event-codes.h의 BTN_LEFT.
const BTN_LEFT: u32 = 0x110;

enum Ready {
    Paths(Vec<PathBuf>),
    Failed,
}

#[derive(Default)]
struct Shared {
    /// 이 창 위에서 누르고 있는 왼쪽 버튼의 serial.
    press: Option<u32>,
    /// 파일이 준비되기 전에 들어온 데이터 요청.
    pending: Vec<(String, OwnedFd)>,
    ready: Option<Ready>,
    events: VecDeque<DragEvent>,
    source: Option<wl_data_source::WlDataSource>,
}

struct State {
    shared: Arc<Mutex<Shared>>,
    surface: wl_surface::WlSurface,
    focused: bool,
    seat: Option<wl_seat::WlSeat>,
    manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    pointer: Option<wl_pointer::WlPointer>,
    repaint: eframe::egui::Context,
}

pub struct WaylandDrag {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: wl_data_device_manager::WlDataDeviceManager,
    device: wl_data_device::WlDataDevice,
    surface: wl_surface::WlSurface,
    shared: Arc<Mutex<Shared>>,
    /// 문서 포털 전송을 쓸 수 있는지. 시작을 늦추지 않도록 따로 확인함.
    portal: Arc<std::sync::atomic::AtomicBool>,
}

/// 창이 Wayland 창이면 끌어 놓기 구현을 만듦. 아니면(X11, Windows) None.
pub fn create(
    handles: &(impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle),
    repaint: eframe::egui::Context,
) -> Option<WaylandDrag> {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    let RawDisplayHandle::Wayland(display) = handles.display_handle().ok()?.as_raw() else {
        return None;
    };
    let RawWindowHandle::Wayland(window) = handles.window_handle().ok()?.as_raw() else {
        return None;
    };
    // 안전성: winit이 만든 wl_display와 wl_surface는 창이 살아 있는 동안 유효함.
    // 앱은 창과 같은 수명으로 이 구현을 들고 있으며, 외부 디스플레이는 닫지 않음.
    let backend = unsafe {
        wayland_backend::sys::client::Backend::from_foreign_display(display.display.as_ptr().cast())
    };
    let conn = Connection::from_backend(backend);
    let surface_id = unsafe {
        wayland_backend::sys::client::ObjectId::from_ptr(
            wl_surface::WlSurface::interface(),
            window.surface.as_ptr().cast(),
        )
    }
    .ok()?;
    let surface = wl_surface::WlSurface::from_id(&conn, surface_id).ok()?;
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut state = State {
        shared: shared.clone(),
        surface: surface.clone(),
        focused: false,
        seat: None,
        manager: None,
        pointer: None,
        repaint,
    };
    conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).ok()?;
    // seat의 포인터 지원 여부를 받음.
    queue.roundtrip(&mut state).ok()?;
    let manager = state.manager.clone()?;
    let device = manager.get_data_device(state.seat.as_ref()?, &qh, ());
    queue.roundtrip(&mut state).ok()?;
    let portal = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let found = portal.clone();
    let _ = std::thread::Builder::new()
        .name("zipelo-portal-check".into())
        .spawn(move || {
            let available = pollster::block_on(ashpd::documents::FileTransfer::new()).is_ok();
            zipelo_ui_memory_app::log_debug!("문서 포털 전송 사용 가능: {available}");
            found.store(available, std::sync::atomic::Ordering::Release);
        });
    zipelo_ui_memory_app::log_debug!(
        "Wayland 끌어 놓기 준비 (데이터 장치 v{})",
        manager.version()
    );
    std::thread::Builder::new()
        .name("zipelo-wayland-drag".into())
        .spawn(move || {
            loop {
                if let Err(error) = queue.blocking_dispatch(&mut state) {
                    zipelo_ui_memory_app::log_warn!("Wayland 끌어 놓기 큐 종료: {error}");
                    break;
                }
            }
        })
        .ok()?;
    Some(WaylandDrag {
        conn,
        qh,
        manager,
        device,
        surface,
        shared,
        portal,
    })
}

impl DragOut for WaylandDrag {
    fn start(&mut self, _: &mut zipelo_ui_memory_app::drag_out::Pump<'_>) -> bool {
        let mut shared = self.shared.lock().unwrap();
        let Some(serial) = shared.press else {
            zipelo_ui_memory_app::log_debug!("끌기 시작 실패: 누르고 있는 버튼 없음");
            return false;
        };
        if let Some(old) = shared.source.take() {
            old.destroy();
        }
        shared.pending.clear();
        shared.ready = None;
        shared.events.clear();
        let source = self.manager.create_data_source(&self.qh, ());
        source.offer(URI_LIST.into());
        if self.portal.load(std::sync::atomic::Ordering::Acquire) {
            source.offer(PORTAL.into());
        }
        if source.version() >= 3 {
            source.set_actions(wl_data_device_manager::DndAction::Copy);
        }
        self.device
            .start_drag(Some(&source), &self.surface, None, serial);
        shared.source = Some(source);
        drop(shared);
        self.conn.flush().is_ok()
    }

    fn poll(&mut self) -> Option<DragEvent> {
        self.shared.lock().unwrap().events.pop_front()
    }

    fn provide(&mut self, paths: &[PathBuf]) {
        let pending = {
            let mut shared = self.shared.lock().unwrap();
            shared.ready = Some(Ready::Paths(paths.to_vec()));
            std::mem::take(&mut shared.pending)
        };
        for (mime, fd) in pending {
            send(mime, fd, paths.to_vec());
        }
    }

    fn fail(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        shared.ready = Some(Ready::Failed);
        // 기다리던 요청은 fd를 닫아 빈 데이터로 끝냄.
        shared.pending.clear();
    }
}

/// 받는 쪽이 요청한 형식으로 데이터를 써 줌. 받는 쪽이 늦게 읽어도 화면이 멈추지 않게
/// 별도 스레드에서 씀.
fn send(mime: String, fd: OwnedFd, paths: Vec<PathBuf>) {
    let _ = std::thread::Builder::new()
        .name("zipelo-wayland-send".into())
        .spawn(move || {
            let data = if mime == PORTAL {
                match portal_key(&paths) {
                    Ok(key) => key,
                    Err(error) => {
                        zipelo_ui_memory_app::log_warn!("문서 포털 전송 실패: {error}");
                        return;
                    }
                }
            } else {
                uri_list(&paths)
            };
            let mut file = std::fs::File::from(fd);
            if let Err(error) = file.write_all(data.as_bytes()) {
                zipelo_ui_memory_app::log_warn!("끌어 놓기 데이터 쓰기 실패: {error}");
            }
        });
}

/// 문서 포털에 파일을 올리고 받는 쪽에 넘길 전송 키를 만듦.
fn portal_key(paths: &[PathBuf]) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let files = paths
        .iter()
        .map(|path| {
            // 폴더도 넘길 수 있도록 O_PATH로 엶.
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(0o10000000 | 0o2000000)
                .open(path)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    pollster::block_on(async {
        let transfer = ashpd::documents::FileTransfer::new().await?;
        let key = transfer.start_transfer(false, true).await?;
        transfer.add_files(&key, &files).await?;
        Ok::<_, ashpd::Error>(key)
    })
    .map_err(|error| error.to_string())
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(5), qh, ()));
                }
                "wl_data_device_manager" if state.manager.is_none() => {
                    state.manager = Some(registry.bind(name, version.min(3), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
            && capabilities.contains(wl_seat::Capability::Pointer)
            && state.pointer.is_none()
        {
            state.pointer = Some(seat.get_pointer(qh, ()));
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface, .. } => {
                state.focused = surface == state.surface;
            }
            wl_pointer::Event::Leave { .. } => state.focused = false,
            wl_pointer::Event::Button {
                serial,
                button: BTN_LEFT,
                state: WEnum::Value(pressed),
                ..
            } => {
                state.shared.lock().unwrap().press = (state.focused
                    && pressed == wl_pointer::ButtonState::Pressed)
                    .then_some(serial);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_data_device_manager::WlDataDeviceManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_data_device_manager::WlDataDeviceManager,
        _: wl_data_device_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_data_device::WlDataDevice,
        _: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // 이 창으로 들어오는 끌어 놓기는 winit이 따로 처리하므로 무시함.
    }

    event_created_child!(State, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        _: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for State {
    fn event(
        state: &mut Self,
        source: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut shared = state.shared.lock().unwrap();
        let current = shared.source.as_ref() == Some(source);
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                zipelo_ui_memory_app::log_debug!("끌어 놓기 데이터 요청: {mime_type}");
                match &shared.ready {
                    Some(Ready::Paths(paths)) => send(mime_type, fd, paths.clone()),
                    // 실패했으면 fd를 닫아 빈 데이터로 끝냄.
                    Some(Ready::Failed) => {}
                    None => shared.pending.push((mime_type, fd)),
                }
            }
            wl_data_source::Event::Cancelled => {
                source.destroy();
                if current {
                    shared.source = None;
                    shared.events.push_back(DragEvent::Cancelled);
                }
            }
            wl_data_source::Event::DndDropPerformed if current => {
                shared.events.push_back(DragEvent::Dropped);
            }
            wl_data_source::Event::DndFinished => {
                source.destroy();
                if current {
                    shared.source = None;
                    shared.events.push_back(DragEvent::Finished);
                }
            }
            _ => {}
        }
        drop(shared);
        state.repaint.request_repaint();
    }
}
