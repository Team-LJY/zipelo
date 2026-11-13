use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/assets/icons/zipelo.ico");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var_os("CARGO_FEATURE_UI").is_none()
    {
        return;
    }
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
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let compiled = output.join(if msvc { "zipelo.res" } else { "zipelo.o" });
    let compiler = env::var("RC").unwrap_or_else(|_| {
        if msvc {
            "rc.exe"
        } else {
            "x86_64-w64-mingw32-windres"
        }
        .into()
    });
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
    assert!(
        command
            .status()
            .expect("Windows 리소스 컴파일러가 필요함")
            .success()
    );
    println!(
        "cargo:rustc-link-arg-bin=zipelo-ui-memory-app={}",
        compiled.display()
    );
}
