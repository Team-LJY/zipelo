use crate::archive_ui::*;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

pub const UNCERTAIN: &str = "저장이 끝났는지 확인하지 못했습니다. 풀 위치 폴더를 확인하세요.";
pub const PARTIAL: &str = "일부 파일만 풀렸습니다. 풀린 파일은 풀 위치 폴더에 남아 있습니다.";
pub const LEFTOVERS: &str =
    "선택한 파일은 모두 저장했지만 임시 파일을 지우지 못했습니다. 풀 위치 폴더를 확인하세요.";
pub const SYNC_WARNING: &str = "파일을 디스크에 완전히 저장했는지 확인하지 못했습니다.";
pub const CLEANUP_WARNING: &str =
    "임시 파일을 지우지 못했습니다. 풀 위치 폴더에 남은 임시 파일이 있을 수 있습니다.";
pub const INTERNAL: &str = "내부 오류가 발생했습니다. 프로그램을 다시 시작해 주세요";
pub const FOLDER_ERROR: &str = "풀 위치 폴더를 만들 수 없습니다. 접근 권한을 확인하세요";

pub fn error_text(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Integrity => "압축파일이 손상됐습니다",
        ErrorCode::UnsupportedFormat => "지원하지 않는 압축 형식입니다",
        ErrorCode::UnsupportedFeature => "아직 지원하지 않는 기능입니다",
        ErrorCode::WrongPassword => "암호가 올바르지 않습니다",
        ErrorCode::UnsafePath => "파일 경로의 안전성을 확인할 수 없어 중단했습니다",
        ErrorCode::Conflict => "같은 이름의 파일이 이미 있습니다. 다른 폴더를 선택하세요",
        ErrorCode::Limit => "처리 한도를 초과했습니다",
        ErrorCode::Io => "파일을 읽거나 쓰지 못했습니다. 파일과 폴더의 접근 권한을 확인하세요",
        ErrorCode::DiskFull => "디스크 여유 공간이 부족합니다. 공간을 확보한 뒤 다시 시도하세요",
        ErrorCode::SourceChanged => "원본 파일이 바뀌었습니다. 압축파일을 다시 여세요",
        ErrorCode::InvalidRequest => "풀 위치와 선택한 항목을 확인하세요",
        ErrorCode::Protocol | ErrorCode::Internal => INTERNAL,
    }
}

pub fn progress_text(stage: Stage, creating: bool) -> &'static str {
    match stage {
        Stage::Scan | Stage::Read => "압축파일을 읽는 중",
        Stage::Decode if creating => "압축하는 중",
        Stage::Decode => "압축을 푸는 중",
        Stage::Write => "파일을 저장하는 중",
        Stage::Verify => "확인하는 중",
        Stage::Commit => "마무리하는 중",
    }
}

pub fn bytes_text(bytes: u64) -> String {
    for (unit, size) in [("GB", 1_u64 << 30), ("MB", 1 << 20), ("KB", 1 << 10)] {
        if bytes >= size {
            return format!("{:.1} {unit}", bytes as f64 / size as f64);
        }
    }
    format!("{bytes} B")
}

pub fn middle_text(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars || max_chars < 5 {
        return text.into();
    }
    let left = (max_chars - 1) / 2;
    chars[..left]
        .iter()
        .chain(std::iter::once(&'…'))
        .chain(chars[chars.len() - (max_chars - left - 1)..].iter())
        .collect()
}

pub fn default_destination(archive: &Path) -> PathBuf {
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_ascii_lowercase();
    let stem = if lower.ends_with(".tar.gz") {
        &name[..name.len() - 7]
    } else {
        name.rsplit_once('.')
            .map_or(name.as_ref(), |(stem, _)| stem)
    };
    archive.with_file_name(if stem.is_empty() {
        "압축 해제"
    } else {
        stem
    })
}

pub fn prepare_destination(path: &Path, automatic: bool) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(false),
        Ok(_) => Err(io::Error::other(FOLDER_ERROR)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && automatic => {
            fs::create_dir(path)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

pub fn remove_owned_empty(path: &Path) {
    // 이번 요청에서 만든 폴더만 호출하며 비어 있지 않으면 보존함.
    let _ = fs::remove_dir(path);
}

pub fn target_files(
    entries: &[Entry],
    selected: &BTreeSet<u32>,
    only: bool,
    gzip: Option<&str>,
) -> Option<BTreeSet<String>> {
    let selected_names = entries
        .iter()
        .filter(|e| selected.contains(&e.id))
        .map(|e| (e.name.as_str(), e.kind))
        .collect::<Vec<_>>();
    let mut names = BTreeSet::new();
    for entry in entries.iter().filter(|e| e.kind == Kind::File) {
        let included = !only
            || selected_names.iter().any(|(name, kind)| {
                *name == entry.name
                    || (*kind == Kind::Directory
                        && entry
                            .name
                            .starts_with(&format!("{}/", name.trim_end_matches('/'))))
            });
        if included && !names.insert(gzip.unwrap_or(&entry.name).replace('\\', "/")) {
            return None;
        }
    }
    Some(names)
}

pub fn warnings(
    terminal: Option<&Terminal>,
    confirmed: bool,
    targets: Option<&BTreeSet<String>>,
    outputs: &[(String, Disposition)],
) -> Vec<&'static str> {
    let Some(t) = terminal.filter(|_| confirmed) else {
        return vec![UNCERTAIN];
    };
    let created = outputs
        .iter()
        .filter(|(_, state)| *state == Disposition::Created)
        .map(|(name, _)| name.replace('\\', "/"))
        .collect::<BTreeSet<_>>();
    let created_reports = outputs
        .iter()
        .filter(|(_, state)| *state == Disposition::Created)
        .count();
    let uncertain = t.publication == Publication::Uncertain
        || outputs
            .iter()
            .any(|(_, state)| *state == Disposition::CommitUncertain)
        || created.len() != created_reports
        || created_reports as u64 != t.created_count
        || (t.publication == Publication::None && !created.is_empty());
    let retained = t.retained_partial_count > 0
        || outputs
            .iter()
            .any(|(_, state)| *state == Disposition::PartialRetained);
    let mut result = Vec::new();
    let mut all_saved_with_leftovers = false;
    if uncertain {
        result.push(UNCERTAIN);
    } else if let Some(targets) = targets {
        let saved = targets.intersection(&created).count();
        if !targets.is_empty() && saved == targets.len() && retained {
            result.push(LEFTOVERS);
            all_saved_with_leftovers = true;
        } else if saved > 0 && saved < targets.len() && t.outcome != Outcome::Completed {
            result.push(PARTIAL);
        } else if (saved == 0 && !created.is_empty())
            || (retained && saved == 0)
            || (t.outcome == Outcome::Completed && saved != targets.len())
        {
            result.push(UNCERTAIN);
        }
    } else {
        result.push(UNCERTAIN);
    }
    if matches!(t.durability, Durability::Failed | Durability::Unconfirmed) {
        result.push(SYNC_WARNING);
    }
    if (retained || t.cleanup != Cleanup::Complete) && !all_saved_with_leftovers {
        result.push(CLEANUP_WARNING);
    }
    result
}

pub fn definitely_unsaved(
    t: Option<&Terminal>,
    confirmed: bool,
    outputs: &[(String, Disposition)],
) -> bool {
    t.is_some_and(|t| {
        confirmed
            && t.publication == Publication::None
            && t.created_count == 0
            && t.retained_partial_count == 0
            && t.cleanup == Cleanup::Complete
            && outputs
                .iter()
                .all(|(_, state)| *state == Disposition::Removed)
    })
}

#[derive(Clone)]
pub struct BrowserEntry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub link: bool,
}

pub fn directory_entries(path: &Path) -> io::Result<Vec<BrowserEntry>> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("링크 폴더는 탐색하지 않음"));
    }
    let mut entries = Vec::new();
    for item in fs::read_dir(path)? {
        let item = item?;
        let kind = item.file_type()?;
        entries.push(BrowserEntry {
            path: item.path(),
            name: item.file_name().to_string_lossy().into(),
            directory: kind.is_dir(),
            link: kind.is_symlink(),
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn home_directory() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
}

pub fn shortcuts() -> Vec<(String, PathBuf)> {
    let home = home_directory();
    let mut paths = vec![("홈".into(), home.clone())];
    for (label, candidates) in [
        ("바탕 화면", ["Desktop", "바탕화면"]),
        ("문서", ["Documents", "문서"]),
        ("다운로드", ["Downloads", "다운로드"]),
    ] {
        if let Some(path) = candidates
            .iter()
            .map(|name| home.join(name))
            .find(|p| p.is_dir())
        {
            paths.push((label.into(), path));
        }
    }
    #[cfg(windows)]
    for drive in b'A'..=b'Z' {
        let path = PathBuf::from(format!("{}:\\", drive as char));
        if path.is_dir() {
            paths.push((format!("{}:", drive as char), path));
        }
    }
    paths
}

pub fn folder_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    });
    command.arg(path);
    command
}

pub const UNCONFIRMED_FINISH: &str =
    "작업이 정상적으로 끝났는지 확인하지 못했습니다. 풀 위치 폴더를 확인하세요";

pub fn client_error_text(error: &str) -> &'static str {
    if error.starts_with("작업자 종료 결과를 확인할 수 없습니다") {
        UNCONFIRMED_FINISH
    } else {
        INTERNAL
    }
}

pub fn activity_text(model: &Model, creating: bool) -> &'static str {
    if model.terminal.is_some()
        || matches!(
            model.cancel,
            Some(CancelStatus::TooLate | CancelStatus::Finished)
        )
    {
        "마무리하는 중"
    } else if model.cancellation_requested() || model.cancel.is_some() {
        "취소하는 중..."
    } else if model.challenge.is_some() {
        "암호를 입력하세요"
    } else {
        model
            .progress
            .map_or("압축파일을 읽는 중", |(stage, _, _, _)| {
                progress_text(stage, creating)
            })
    }
}
