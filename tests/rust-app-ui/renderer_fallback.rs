#[path = "../../experiments/rust-ui-memory-app/src/renderer.rs"]
#[allow(dead_code)]
mod renderer;
use renderer::{Backend, try_backends};

#[test]
fn windows_tries_hardware_then_software_then_glow() {
    let mut seen = Vec::new();
    let result = try_backends(true, |backend| {
        seen.push(backend);
        Err((backend, true))
    });
    assert_eq!(
        seen,
        [Backend::Dx12Hardware, Backend::Dx12Software, Backend::Glow]
    );
    assert_eq!(result, Err(Backend::Glow));
}

#[test]
fn successful_software_does_not_try_glow() {
    let mut seen = Vec::new();
    assert_eq!(
        try_backends(true, |backend| {
            seen.push(backend);
            if backend == Backend::Dx12Hardware {
                Err(((), true))
            } else {
                Ok(())
            }
        }),
        Ok(())
    );
    assert_eq!(seen, [Backend::Dx12Hardware, Backend::Dx12Software]);
}

#[test]
fn successful_hardware_stops_retries() {
    let mut seen = Vec::new();
    assert_eq!(
        try_backends(true, |backend| {
            seen.push(backend);
            Ok::<_, ((), bool)>(())
        }),
        Ok(())
    );
    assert_eq!(seen, [Backend::Dx12Hardware]);
}

#[test]
fn running_app_failure_is_not_restarted() {
    let mut seen = Vec::new();
    assert_eq!(
        try_backends(true, |backend| {
            seen.push(backend);
            Err(("앱 생성 후", false))
        }),
        Err("앱 생성 후")
    );
    assert_eq!(seen, [Backend::Dx12Hardware]);
}

#[test]
fn linux_only_tries_glow() {
    let mut seen = Vec::new();
    assert_eq!(
        try_backends(false, |backend| {
            seen.push(backend);
            Err(((), true))
        }),
        Err(())
    );
    assert_eq!(seen, [Backend::Glow]);
}

#[test]
fn options_keep_event_loop_available_for_retry() {
    let options = renderer::options(Backend::Glow, eframe::NativeOptions::default());
    assert!(options.run_and_return);
    assert!(matches!(options.renderer, eframe::Renderer::Glow));
}
