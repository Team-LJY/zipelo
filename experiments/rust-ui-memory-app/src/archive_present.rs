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
        ErrorCode::RecoveryIncomplete => "일부 항목만 복구했습니다. 복구본과 경고를 확인하세요.",
        ErrorCode::RecoveryImpossible => "복구 가능한 항목이 없습니다.",
        ErrorCode::MissingVolume => {
            "분할 압축파일의 조각이 없습니다. 모든 조각을 같은 폴더에 모은 뒤 .001을 다시 여세요"
        }
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

/// 자원 상한 표시. u64::MAX는 상한이 없다는 뜻임.
pub fn limit_text(bytes: u64) -> String {
    if bytes == u64::MAX {
        "제한 없음".into()
    } else {
        bytes_text(bytes)
    }
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
    let compound = [".tar.bz2", ".tar.gz", ".tar.xz"]
        .into_iter()
        .find(|suffix| lower.ends_with(suffix));
    let stem = if let Some(suffix) = compound {
        &name[..name.len() - suffix.len()]
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

pub fn volume_problem_text(problem: &VolumeProblem) -> String {
    fn visible(c: char) -> String {
        if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            c.escape_unicode().to_string()
        } else {
            c.to_string()
        }
    }
    let name = match &problem.volume {
        NativePath::Unix(bytes) => bytes
            .utf8_chunks()
            .map(|chunk| {
                let mut text = chunk.valid().chars().map(visible).collect::<String>();
                for byte in chunk.invalid() {
                    text.push_str(&format!("\\x{byte:02x}"));
                }
                text
            })
            .collect::<String>(),
        NativePath::Windows(units) => char::decode_utf16(units.iter().copied())
            .map(|c| {
                c.map(visible)
                    .unwrap_or_else(|e| format!("\\u{{{:04x}}}", e.unpaired_surrogate()))
            })
            .collect::<String>(),
    };
    match problem.reason {
        VolumeProblemReason::FirstVolumeRequired => format!("첫 조각 {name}을 선택해 다시 여세요."),
        VolumeProblemReason::MissingVolume => {
            format!("{name} 조각이 없습니다. 모든 조각을 같은 폴더에 모은 뒤 .001을 다시 여세요.")
        }
        VolumeProblemReason::IncompleteSet => format!(
            "{name}의 분할 묶음이 불완전하거나 손상됐습니다. 모든 조각의 존재와 손상을 확인한 뒤 .001을 다시 여세요."
        ),
    }
}

pub const SPLIT_SUPPORT: &str =
    "분할 7z는 .7z.001부터 열 수 있습니다. 분할 ZIP과 RAR는 지원하지 않습니다.";

/// 7z 분할 볼륨 크기 단위. 크기 표시(bytes_text)와 같이 1 MB = 1,048,576바이트.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeUnit {
    Mb,
    Gb,
}
impl VolumeUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mb => "MB",
            Self::Gb => "GB",
        }
    }
    fn bytes(self) -> u64 {
        match self {
            Self::Mb => 1 << 20,
            Self::Gb => 1 << 30,
        }
    }
}
/// 분할 7z 볼륨 수 상한. 분할 읽기는 `.001`부터 `.999`까지만 찾음.
pub const MAX_VOLUMES: u64 = worker_protocol::protocol::MAX_SEVEN_ZIP_VOLUMES;
pub const VOLUME_SIZE_ERROR: &str = "쪼갤 용량은 1 이상의 정수로 입력하세요";
pub const VOLUME_LIMIT_ERROR: &str =
    "볼륨이 999개를 넘거나 처리 한도를 넘었습니다. 쪼갤 용량을 늘려 다시 만드세요";

/// 입력한 숫자와 단위를 볼륨 바이트 수로 바꿈. 최소 1 MB이고 정수만 받음.
pub fn volume_size(amount: &str, unit: VolumeUnit) -> Result<u64, &'static str> {
    let amount = amount.trim();
    if amount.is_empty() || amount.len() > 9 || !amount.bytes().all(|b| b.is_ascii_digit()) {
        return Err(VOLUME_SIZE_ERROR);
    }
    amount
        .parse::<u64>()
        .ok()
        .and_then(|value| value.checked_mul(unit.bytes()))
        .filter(|bytes| *bytes >= worker_protocol::protocol::MIN_SEVEN_ZIP_VOLUME_BYTES)
        .ok_or(VOLUME_SIZE_ERROR)
}

/// 분할 7z의 `index`번째 볼륨 경로 (`이름.7z.001` 형식).
pub fn volume_path(output: &Path, index: u64) -> PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(format!(".{index:03}"));
    PathBuf::from(name)
}

/// 같은 폴더에 분할 읽기가 한 묶음으로 볼 `이름.7z.<숫자>`가 이미 있으면 그 이름.
pub fn existing_volume(output: &Path) -> Option<String> {
    let parent = output.parent()?;
    let prefix = format!("{}.", output.file_name()?.to_str()?);
    let fold = |value: &str| {
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value.to_owned()
        }
    };
    let prefix = fold(&prefix);
    fs::read_dir(parent).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let suffix = fold(&name).strip_prefix(&prefix)?.to_owned();
        (!suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())).then_some(name)
    })
}

pub fn excluded_split(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    name.ends_with(".zip.001") || name.ends_with(".z01") || name.ends_with(".rar")
}

/// 같은 이름이 이미 있으면 `이름 (1)`, `이름 (2)`처럼 비어 있는 이름을 찾음.
pub fn unique_folder(base: &Path) -> PathBuf {
    let taken = |path: &Path| fs::symlink_metadata(path).is_ok();
    if !taken(base) {
        return base.to_path_buf();
    }
    let name = base.file_name().unwrap_or_default().to_string_lossy();
    (1..)
        .map(|n| base.with_file_name(format!("{name} ({n})")))
        .find(|path| !taken(path))
        .unwrap()
}

fn private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

pub struct OpenRoot {
    path: PathBuf,
    handle: fs::File,
    identity: worker_protocol::protocol::SourceObject,
}

impl OpenRoot {
    fn capture(path: PathBuf) -> io::Result<Self> {
        let handle = open_root_handle(&path)?;
        // 만든 직후에는 비어 있어야 함. 만든 뒤 핸들을 잡기 전에 다른 폴더로 바뀌었다면
        // 사용자 데이터가 들어 있을 수 있으므로 원본으로 기록하지 않음.
        if !worker_protocol::tree_remove::is_empty(&handle, &path)? {
            return Err(io::Error::other("만든 임시 폴더가 아님"));
        }
        let identity = worker_protocol::filesystem::object(&handle)
            .map_err(|_| io::Error::other("임시 폴더 식별 실패"))?;
        Ok(Self {
            path,
            handle,
            identity,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_current(&self) -> bool {
        // 원본 핸들을 유지해 삭제된 inode가 재사용되어도 같다고 판정하지 않음.
        self.handle.metadata().is_ok()
            && open_root_handle(&self.path)
                .ok()
                .and_then(|file| worker_protocol::filesystem::object(&file).ok())
                .is_some_and(|identity| identity == self.identity)
    }
}

fn open_root_handle(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000); // 심볼릭 링크를 따라가지 않음.
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // 폴더 자체를 열며 재분석 지점을 따라가지 않음.
        // 삭제 공유를 허용하지 않아 열어 둔 동안 폴더 이름을 바꾸거나 지울 수 없게 함.
        options.share_mode(1 | 2).custom_flags(0x02000000 | 0x00200000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("임시 폴더가 아님"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("재분석 지점은 임시 폴더로 쓰지 않음"));
        }
    }
    Ok(file)
}

/// 압축파일 안 파일을 기본 프로그램으로 열 때 쓸 임시 폴더를 만듦.
/// 압축파일과 같은 폴더에 만들고, 그곳에 만들 수 없으면 시스템 임시 폴더를 씀.
/// 숨김 폴더로 만들지 않음. snap으로 설치한 Firefox 등은 홈 바로 아래 숨김 폴더와
/// 시스템 임시 폴더의 파일을 읽지 못해 열기 요청을 조용히 무시함.
pub fn create_open_root(archive: &Path) -> io::Result<OpenRoot> {
    create_open_root_with(archive, |_| {})
}

/// `create_open_root`와 같고, 폴더를 만든 직후 핸들을 잡기 전에 `after_create`를 실행함.
/// 만든 뒤 경로가 바뀌는 경우를 시험하려고 둠.
/// 폴더를 만든 뒤 핸들을 잡기까지의 틈에 경로가 바뀌어도, 핸들로 확인한 폴더가 비어 있지
/// 않으면 원본으로 쓰지 않고 다른 이름으로 다시 만듦. 바뀐 폴더는 건드리지 않음.
pub fn create_open_root_with(
    archive: &Path,
    mut after_create: impl FnMut(&Path),
) -> io::Result<OpenRoot> {
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let mut error = None;
    // Windows는 점으로 숨겨지지 않으므로 숨김 속성을 줌.
    if let Some(parent) = archive.parent().filter(|p| !p.as_os_str().is_empty()) {
        let base = parent.join(open_root_name_in(parent, &name));
        for _ in 0..8 {
            let path = unique_folder(&base);
            match private_dir(&path) {
                Ok(()) => {
                    #[cfg(windows)]
                    hide_folder(&path);
                    after_create(&path);
                    match OpenRoot::capture(path.clone()) {
                        Ok(root) => return Ok(root),
                        Err(e) => {
                            // 비어 있는 폴더만 지워지므로 바뀐 폴더의 내용은 남음.
                            let _ = fs::remove_dir(&path);
                            error = Some(e);
                            continue;
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
    }
    let base = std::env::temp_dir().join(format!("zipelo-{}-{name}", std::process::id()));
    let path = unique_folder(&base);
    private_dir(&path).map_err(|e| error.unwrap_or(e))?;
    after_create(&path);
    OpenRoot::capture(path.clone()).inspect_err(|_| {
        let _ = fs::remove_dir(&path);
    })
}

/// Windows 탐색기에서 임시 폴더가 압축파일 옆에 보이지 않도록 숨김 속성을 줌.
/// 실패해도 폴더는 그대로 씀. unsafe 없이 시스템의 attrib.exe를 창 없이 실행함.
#[cfg(windows)]
pub fn hide_folder(path: &Path) {
    use std::os::windows::process::CommandExt;
    let Some(system) = std::env::var_os("SystemRoot").map(PathBuf::from) else {
        return;
    };
    let _ = std::process::Command::new(system.join("System32").join("attrib.exe"))
        .arg("+h")
        .arg(path)
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 열기와 끌어 놓기용 임시 폴더 이름. 예: `.자료.zip (Zipelo 임시)`.
/// 파일 관리자에서 압축파일 옆에 풀린 것처럼 보이지 않도록 Unix 계열에서는 숨김
/// 이름(앞에 '.')을 씀. Windows는 점으로 숨겨지지 않으므로 원래 이름에 숨김 속성을 줌.
pub fn open_root_name(archive_name: &str) -> String {
    if cfg!(windows) {
        format!("{archive_name} (Zipelo 임시)")
    } else {
        format!(".{archive_name} (Zipelo 임시)")
    }
}

/// `parent` 폴더에 만들 임시 폴더 이름. snap 앱(Firefox 등)은 홈 폴더 바로 아래의
/// 숨김 항목을 읽지 못하므로, 압축파일이 홈 폴더 바로 아래에 있으면 숨기지 않음.
/// 그보다 아래 폴더의 숨김 항목은 snap 앱도 읽을 수 있음.
pub fn open_root_name_in(parent: &Path, archive_name: &str) -> String {
    let home = home_directory();
    let same = |a: &Path, b: &Path| {
        a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b)
    };
    if cfg!(windows) || same(parent, &home) {
        format!("{archive_name} (Zipelo 임시)")
    } else {
        open_root_name(archive_name)
    }
}

/// 이 앱이 만든 열기용 폴더를 지움. 다른 프로그램이 파일을 쓰고 있으면 남을 수 있음.
pub fn remove_open_root(root: OpenRoot) {
    remove_open_root_with(root, || {});
}

/// `remove_open_root`와 같고, 정체를 확인한 직후 삭제 전에 `after_check`를 실행함.
/// 확인과 삭제 사이에 경로가 바뀌는 경우를 시험하려고 둠.
/// 내용은 만들 때 열어 둔 폴더 핸들에 묶어 지우므로 경로가 바뀌어도 다른 폴더는 건드리지 않음.
pub fn remove_open_root_with(root: OpenRoot, after_check: impl FnOnce()) {
    if !root.is_current() {
        return;
    }
    after_check();
    let OpenRoot { path, handle, .. } = root;
    let _ = worker_protocol::tree_remove::remove_contents(&handle, &path);
    // Windows는 핸들을 닫아야 폴더를 지울 수 있음.
    drop(handle);
    // 비어 있지 않거나 링크로 바뀐 경로는 지워지지 않으므로 다른 내용이 사라지지 않음.
    let _ = fs::remove_dir(&path);
}

/// 기본 프로그램으로 열면 실행될 수 있는 파일은 바로 열지 않음.
/// Windows는 스크립트도 연결 프로그램이 바로 실행하므로 범위가 넓고,
/// Linux는 스크립트를 편집기로 열므로 실행·설치 파일만 막음.
pub fn blocked_open(name: &str) -> bool {
    #[cfg(windows)]
    const EXECUTABLE: &[&str] = &[
        "exe", "com", "bat", "cmd", "scr", "pif", "msi", "msp", "msc", "ps1", "psm1", "vbs",
        "vbe", "js", "jse", "wsf", "wsh", "hta", "cpl", "lnk", "url", "reg", "jar", "inf", "scf",
        "chm", "dll", "sys", "appref-ms", "application", "gadget", "settingcontent-ms",
    ];
    #[cfg(not(windows))]
    const EXECUTABLE: &[&str] = &[
        "desktop", "appimage", "run", "bin", "jar", "deb", "rpm", "snap", "flatpakref",
        "flatpak", "exe", "msi",
    ];
    open_extension(name).is_some_and(|ext| EXECUTABLE.contains(&ext.to_ascii_lowercase().as_str()))
}

fn open_extension(name: &str) -> Option<&str> {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
    leaf.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty())
}

/// 실행 파일이라 열지 않았다는 짧은 안내. 압축은 풀지 않음.
pub fn open_blocked_text(name: &str) -> String {
    match open_extension(name) {
        Some(ext) => format!("실행 파일(.{ext})은 안전을 위해 바로 열 수 없습니다"),
        None => "실행 파일은 안전을 위해 바로 열 수 없습니다".into(),
    }
}

pub const OPEN_FAILED: &str = "파일을 열 프로그램을 실행하지 못했습니다";
pub const OPEN_MISSING: &str = "풀린 파일을 찾지 못해 열지 못했습니다";


/// 운영체제에 설정된 기본 프로그램으로 파일을 엶.
pub fn open_with_default(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(path);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().map_err(|_| OPEN_FAILED.to_owned())?;
    // 끝난 자식 프로세스를 거둬 좀비로 남지 않게 함.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
