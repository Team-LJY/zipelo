use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purpose {
    Open,
    Files,
    Folder,
    Destination,
    Retry,
    SaveZip,
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

#[derive(Default)]
pub struct NativeDialogs;

impl Dialogs for NativeDialogs {
    fn choose(&mut self, request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String> {
        let dialog = rfd::FileDialog::new().set_directory(request.directory);
        let answer =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request.purpose {
                Purpose::Open => dialog
                    .set_title("압축파일 열기")
                    .add_filter("압축파일", &["zip", "7z", "tar", "gz", "tgz"])
                    .pick_file()
                    .map(|path| vec![path]),
                Purpose::Files => dialog.set_title("파일 추가").pick_files(),
                Purpose::Folder => dialog
                    .set_title("폴더 추가")
                    .pick_folder()
                    .map(|path| vec![path]),
                Purpose::Destination | Purpose::Retry => dialog
                    .set_title("압축을 풀 폴더 선택")
                    .pick_folder()
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
