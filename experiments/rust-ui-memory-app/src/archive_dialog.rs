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

pub type DialogAnswer = Result<Option<Vec<PathBuf>>, String>;

pub enum PendingDialog {
    Ready(DialogAnswer),
    Waiting(std::sync::mpsc::Receiver<DialogAnswer>),
}

pub trait Dialogs {
    fn choose(&mut self, request: DialogRequest) -> DialogAnswer;

    /// 선택 창을 연다. 기본 구현은 응답을 바로 돌려준다.
    fn start(&mut self, request: DialogRequest) -> PendingDialog {
        PendingDialog::Ready(self.choose(request))
    }
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

pub const DIALOG_UNAVAILABLE: &str =
    "파일 선택 창을 열 수 없습니다. 파일을 창으로 끌어 놓아 주세요.";

impl Dialogs for NativeDialogs {
    fn choose(&mut self, request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(target_os = "linux")]
            {
                choose_portal(request)
            }
            #[cfg(not(target_os = "linux"))]
            {
                choose_native(request)
            }
        }))
        .unwrap_or_else(|_| Err(DIALOG_UNAVAILABLE.into()))
    }

    // 포털 응답을 화면 스레드에서 기다리면 창이 이벤트를 처리하지 못해 "응답 없음"으로 표시됨.
    #[cfg(target_os = "linux")]
    fn start(&mut self, request: DialogRequest) -> PendingDialog {
        let (sender, receiver) = std::sync::mpsc::channel();
        match std::thread::Builder::new()
            .name("zipelo-file-dialog".into())
            .spawn(move || {
                let _ = sender.send(NativeDialogs.choose(request));
            }) {
            Ok(_) => PendingDialog::Waiting(receiver),
            Err(_) => PendingDialog::Ready(Err(DIALOG_UNAVAILABLE.into())),
        }
    }
}

#[cfg(target_os = "linux")]
pub fn portal_response(
    response: Result<Vec<PathBuf>, ashpd::Error>,
) -> Result<Option<Vec<PathBuf>>, String> {
    match response {
        Ok(paths) if !paths.is_empty() => Ok(Some(paths)),
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
        | Err(ashpd::Error::Portal(ashpd::PortalError::Cancelled(_))) => Ok(None),
        _ => Err(DIALOG_UNAVAILABLE.into()),
    }
}

#[cfg(target_os = "linux")]
fn choose_portal(request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String> {
    use ashpd::desktop::file_chooser::{FileFilter, OpenFileRequest, SaveFileRequest};
    let filters = match request.purpose {
        Purpose::Open => archive_filters(),
        Purpose::SaveZip => vec![("ZIP 압축파일", vec!["zip".into()])],
        _ => Vec::new(),
    }
    .into_iter()
    .map(|(name, extensions)| {
        extensions
            .into_iter()
            .fold(FileFilter::new(name), |filter, extension| {
                let glob = if extension == "*" {
                    "*".to_owned()
                } else {
                    format!("*.{extension}")
                };
                filter.glob(&glob)
            })
    })
    .collect::<Vec<_>>();
    portal_response(pollster::block_on(async move {
        let selected = match request.purpose {
            Purpose::SaveZip | Purpose::SaveGzip | Purpose::RetryGzip => SaveFileRequest::default()
                .title(if request.purpose == Purpose::SaveZip {
                    "ZIP 파일 저장"
                } else {
                    "압축 해제 파일 저장"
                })
                .current_name(Some(request.file_name.as_str()))
                .filters(filters)
                .current_folder::<&PathBuf>(Some(&request.directory))?
                .send()
                .await?
                .response()?,
            purpose => OpenFileRequest::default()
                .title(match purpose {
                    Purpose::Open => "압축파일 열기",
                    Purpose::Files => "파일 추가",
                    Purpose::Folder => "폴더 추가",
                    _ => "압축을 풀 폴더 선택",
                })
                .multiple(purpose == Purpose::Files)
                .directory(matches!(
                    purpose,
                    Purpose::Folder | Purpose::Destination | Purpose::Retry
                ))
                .filters(filters)
                .current_folder::<&PathBuf>(Some(&request.directory))?
                .send()
                .await?
                .response()?,
        };
        selected
            .uris()
            .iter()
            .map(|uri| {
                uri.to_file_path()
                    .map_err(|_| ashpd::Error::ParseError("파일 경로 응답 오류"))
            })
            .collect::<Result<Vec<_>, _>>()
    }))
}

#[cfg(not(target_os = "linux"))]
fn choose_native(request: DialogRequest) -> Result<Option<Vec<PathBuf>>, String> {
    let dialog = rfd::FileDialog::new().set_directory(request.directory);
    let answer = match request.purpose {
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
    };
    Ok(answer)
}
