use crate::archive_ui::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

pub const UNCERTAIN: &str = "저장이 끝났는지 확인하지 못했습니다. 풀 위치 폴더를 확인하세요.";
pub const PARTIAL: &str = "일부 파일만 풀렸습니다. 풀린 파일은 풀 위치 폴더에 남아 있습니다.";
pub const LEFTOVERS: &str =
    "선택한 파일은 모두 저장했지만 임시 파일을 지우지 못했습니다. 풀 위치 폴더를 확인하세요.";
pub const SYNC_FAILED: &str =
    "파일을 디스크에 완전히 저장하지 못했습니다. 풀 위치 폴더를 확인하세요.";
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
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(path)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

pub fn remove_owned_empty(path: &Path) {
    // 이번 요청에서 만든 폴더만 호출하며 비어 있지 않으면 보존함.
    let _ = fs::remove_dir(path);
}

fn requested(entry: &Entry, selection: &Selection) -> bool {
    match selection {
        Selection::All => true,
        Selection::EntryIds(ids) => ids.contains(&entry.id),
    }
}

pub fn target_files(
    entries: &[Entry],
    selection: &Selection,
    gzip: Option<&str>,
) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == Kind::File && requested(entry, selection))
    {
        if !names.insert(gzip.unwrap_or(&entry.name).replace('\\', "/")) {
            return None;
        }
    }
    Some(names)
}

pub fn resolved_targets(
    targets: Option<&BTreeSet<String>>,
    entries: &[Entry],
    gzip: Option<&str>,
    resolutions: &[Resolution],
) -> Option<BTreeSet<String>> {
    let mut names = targets?.clone();
    let entries: BTreeMap<_, _> = entries.iter().map(|entry| (entry.id, entry)).collect();
    for resolution in resolutions {
        let entry = entries.get(&resolution.entry_id)?;
        let original = gzip.unwrap_or(&entry.name).replace('\\', "/");
        if !names.remove(&original) {
            return None;
        }
    }
    for resolution in resolutions {
        if resolution.choice != ConflictChoice::Skip
            && !names.insert(resolution.relative_name.replace('\\', "/"))
        {
            return None;
        }
    }
    Some(names)
}

pub fn target_directories(entries: &[Entry], selection: &Selection) -> BTreeSet<String> {
    let mut directories = BTreeSet::new();
    for entry in entries.iter().filter(|entry| {
        matches!(entry.kind, Kind::File | Kind::Directory) && requested(entry, selection)
    }) {
        let normalized = entry.name.replace('\\', "/");
        let name = normalized.trim_end_matches('/');
        if entry.kind == Kind::Directory {
            directories.insert(name.to_owned());
        }
        let mut parent = name;
        while let Some((directory, _)) = parent.rsplit_once('/') {
            directories.insert(directory.to_owned());
            parent = directory;
        }
    }
    directories
}

pub fn durability_warning(durability: Durability) -> Option<&'static str> {
    match durability {
        Durability::Failed => Some(SYNC_FAILED),
        Durability::Unconfirmed => Some(SYNC_WARNING),
        _ => None,
    }
}

pub fn warnings(
    terminal: Option<&Terminal>,
    confirmed: bool,
    targets: Option<&BTreeSet<String>>,
    directories: &BTreeSet<String>,
    outputs: &[(String, Disposition)],
) -> Vec<&'static str> {
    let mut result = publication_warnings(terminal, confirmed, targets, directories, outputs);
    if let Some(warning) = terminal
        .filter(|_| confirmed)
        .and_then(|t| durability_warning(t.durability))
    {
        result.push(warning);
    }
    result
}

pub fn publication_warnings(
    terminal: Option<&Terminal>,
    confirmed: bool,
    targets: Option<&BTreeSet<String>>,
    directories: &BTreeSet<String>,
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
        let files = created
            .difference(directories)
            .cloned()
            .collect::<BTreeSet<_>>();
        let saved = targets.intersection(&files).count();
        let unknown = !targets.is_disjoint(directories)
            || created
                .iter()
                .any(|name| !targets.contains(name) && !directories.contains(name));
        if unknown {
            result.push(UNCERTAIN);
        } else if !targets.is_empty() && saved == targets.len() && retained {
            result.push(LEFTOVERS);
            all_saved_with_leftovers = true;
        } else if saved > 0 && saved < targets.len() && t.outcome != Outcome::Completed {
            result.push(PARTIAL);
        } else if (saved == 0 && !files.is_empty())
            || (retained && saved == 0)
            || (t.outcome == Outcome::Completed && saved != targets.len())
        {
            result.push(UNCERTAIN);
        }
    } else {
        result.push(UNCERTAIN);
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
    } else if model.conflict.is_some() {
        "파일 충돌 선택 대기 중"
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

pub fn valid_output_leaf(name: &str) -> bool {
    valid_leaf(name) && worker_protocol::archive::valid_name(name, false).is_ok()
}

pub fn gzip_output_name(archive: &Path) -> String {
    let file_name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let candidate = file_name
        .rsplit_once('.')
        .filter(|(_, suffix)| suffix.eq_ignore_ascii_case("gz"))
        .map_or(file_name, |(stem, _)| stem);
    if valid_output_leaf(candidate) {
        candidate.to_owned()
    } else {
        "압축 해제 파일".into()
    }
}

pub fn operation_text(text: &str, creating: bool) -> String {
    if creating {
        text.replace("풀 위치", "저장 위치")
            .replace("선택한 항목", "추가한 항목")
            .replace(
                "일부 파일만 풀렸습니다. 풀린 파일은",
                "ZIP 파일이 일부만 저장됐습니다. 저장된 파일은",
            )
    } else {
        text.to_owned()
    }
}
