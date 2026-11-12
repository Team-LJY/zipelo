use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purpose {
    Open,
    Files,
    Folder,
    Destination,
    Retry,
    SaveZip,
    SaveGzip,
    RetryGzip,
}

#[derive(Clone, Debug)]
pub struct DialogRequest {
    pub purpose: Purpose,
    pub directory: PathBuf,
    pub file_name: String,
}

pub trait Dialogs {
    fn choose(&mut self, request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String>;
}

pub fn archive_filters() -> Vec<(&'static str, Vec<String>)> {
    let mut extensions = Vec::new();
    for extension in ["zip", "7z", "tar", "gz", "tgz"] {
        let mut variants = vec![String::new()];
        for character in extension.chars() {
            let mut next = Vec::new();
            for prefix in variants {
                next.push(format!("{prefix}{character}"));
                if character.is_ascii_alphabetic() {
                    next.push(format!("{prefix}{}", character.to_ascii_uppercase()));
                }
            }
            variants = next;
        }
        extensions.extend(variants);
    }
    vec![("압축파일", extensions), ("모든 파일", vec!["*".into()])]
}

#[derive(Default)]
pub struct NativeDialogs;

impl Dialogs for NativeDialogs {
    fn choose(&mut self, request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String> {
        let dialog = rfd::FileDialog::new().set_directory(request.directory);
        let answer =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request.purpose {
                Purpose::Open => {
                    let mut dialog = dialog.set_title("압축파일 열기");
                    for (name, extensions) in archive_filters() {
                        dialog = dialog.add_filter(name, &extensions);
                    }
                    dialog.pick_file().map(|path| vec![path])
                }
                Purpose::Files => dialog.set_title("파일 추가").pick_files(),
                Purpose::Folder => dialog
                    .set_title("폴더 추가")
                    .pick_folder()
                    .map(|path| vec![path]),
                Purpose::Destination | Purpose::Retry => dialog
                    .set_title("압축을 풀 폴더 선택")
                    .pick_folder()
                    .map(|path| vec![path]),
                Purpose::SaveGzip | Purpose::RetryGzip => dialog
                    .set_title("압축 해제 파일 저장")
                    .set_file_name(request.file_name)
                    .save_file()
                    .map(|path| vec![path]),
                Purpose::SaveZip => dialog
                    .set_title("ZIP 파일 저장")
                    .set_file_name(request.file_name)
                    .add_filter("ZIP 압축파일", &["zip"])
                    .save_file()
                    .map(|path| vec![path]),
            }));
        answer.map_err(|_| "파일 선택 창을 열지 못했습니다. 다시 시도하세요".to_owned())
    }
}
