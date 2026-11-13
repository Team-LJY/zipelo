use std::{env, ffi::OsString, path::PathBuf, process::Command};

fn msvc_compiler() -> Option<OsString> {
    if let Some(compiler) = env::var_os("RC") {
        return Some(compiler);
    }
    let mut candidates = Vec::new();
    for root in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Some(root) = env::var_os(root) else {
            continue;
        };
        let bin = PathBuf::from(root).join("Windows Kits/10/bin");
        let Ok(entries) = std::fs::read_dir(bin) else {
            continue;
        };
        for entry in entries.flatten() {
            let version = entry
                .file_name()
                .to_string_lossy()
                .split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>();
            let compiler = entry.path().join("x64/rc.exe");
            if let Ok(version) = version {
                if compiler.is_file() {
                    candidates.push((version, compiler));
                }
            }
        }
    }
    candidates
        .into_iter()
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path.into_os_string())
        .or_else(|| {
            env::var_os("PATH").and_then(|paths| {
                env::split_paths(&paths)
                    .map(|path| path.join("rc.exe"))
                    .find(|path| path.is_file())
                    .map(PathBuf::into_os_string)
            })
        })
}

fn main() {
    println!("cargo:rerun-if-changed=src/assets/icons/zipelo.ico");
    for variable in ["RC", "ProgramFiles(x86)", "ProgramFiles", "PATH"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var_os("CARGO_FEATURE_UI").is_none()
    {
        return;
    }
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let compiler = if msvc {
        let Some(compiler) = msvc_compiler() else {
            println!("cargo:warning=아이콘 없이 빌드: MSVC 리소스 컴파일러를 찾지 못함");
            return;
        };
        compiler
    } else {
        env::var_os("RC").unwrap_or_else(|| "x86_64-w64-mingw32-windres".into())
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("src/assets/icons/zipelo.ico");
    let resource = output.join("zipelo.rc");
    std::fs::write(
        &resource,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let compiled = output.join(if msvc { "zipelo.res" } else { "zipelo.o" });
    let mut command = Command::new(compiler);
    if msvc {
        command
            .arg("/nologo")
            .arg(format!("/fo{}", compiled.display()))
            .arg(&resource);
    } else {
        if let Ok(preprocessor) = env::var("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER") {
            command.arg(format!("--preprocessor={preprocessor}")).args([
                "--preprocessor-arg=-E",
                "--preprocessor-arg=-xc",
                "--preprocessor-arg=-DRC_INVOKED",
            ]);
        }
        command
            .arg("-i")
            .arg(&resource)
            .arg("-o")
            .arg(&compiled)
            .arg("-O")
            .arg("coff");
    }
    let status = command.status();
    if msvc && !status.as_ref().is_ok_and(|status| status.success()) {
        println!("cargo:warning=아이콘 없이 빌드: MSVC 리소스 컴파일러 실행 실패");
        return;
    }
    assert!(status.expect("Windows 리소스 컴파일러가 필요함").success());
    println!(
        "cargo:rustc-link-arg-bin=zipelo-ui-memory-app={}",
        compiled.display()
    );
}
