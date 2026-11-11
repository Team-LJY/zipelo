#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Dx12Hardware,
    Dx12Software,
    Glow,
}

pub fn try_backends<E>(
    windows: bool,
    mut attempt: impl FnMut(Backend) -> Result<(), (E, bool)>,
) -> Result<(), E> {
    let order: &[Backend] = if windows {
        &[Backend::Dx12Hardware, Backend::Dx12Software, Backend::Glow]
    } else {
        &[Backend::Glow]
    };
    let mut failure = None;
    for &backend in order {
        match attempt(backend) {
            Ok(()) => return Ok(()),
            Err((error, retry)) => {
                if !retry {
                    return Err(error);
                }
                failure = Some(error);
            }
        }
    }
    Err(failure.expect("렌더러 시도 목록은 비어 있지 않음"))
}

pub fn options(backend: Backend, mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    options.renderer = eframe::Renderer::Glow;
    options.run_and_return = true;
    #[cfg(target_os = "windows")]
    if backend != Backend::Glow {
        use eframe::{egui_wgpu, wgpu};
        use std::sync::Arc;
        let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        setup
            .instance_descriptor
            .backend_options
            .dx12
            .shader_compiler = wgpu::Dx12Compiler::Fxc;
        setup.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
            adapters
                .iter()
                .find(|adapter| {
                    let software = adapter.get_info().device_type == wgpu::DeviceType::Cpu;
                    software == (backend == Backend::Dx12Software)
                        && surface.is_none_or(|surface| {
                            !surface.get_capabilities(adapter).formats.is_empty()
                        })
                })
                .cloned()
                .ok_or_else(|| "사용 가능한 DirectX 화면 장치가 없음".into())
        }));
        options.renderer = eframe::Renderer::Wgpu;
        options.wgpu_options.wgpu_setup = egui_wgpu::WgpuSetup::CreateNew(setup);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = backend;
    options
}

pub fn run_native(
    title: &str,
    native_options: eframe::NativeOptions,
    mut create: impl FnMut() -> eframe::AppCreator<'static>,
) -> eframe::Result {
    use std::{cell::Cell, rc::Rc};
    try_backends(cfg!(target_os = "windows"), |backend| {
        let started = Rc::new(Cell::new(false));
        let entered = started.clone();
        let creator = create();
        let result = eframe::run_native(
            title,
            options(backend, native_options.clone()),
            Box::new(move |context| {
                entered.set(true);
                creator(context)
            }),
        );
        // 앱 생성 뒤에는 작업이나 창을 중복 실행하지 않음.
        result.map_err(|error| (error, !started.get()))
    })
}
