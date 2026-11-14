use crate::{
    create_sources::Input,
    filesystem,
    protocol::*,
    spool::{Snapshot, Step},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, Metadata},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

const MAX_VOLUMES: usize = 999;
type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug)]
pub(crate) struct Failure {
    pub error: ErrorCode,
    pub problem: Option<(VolumeProblemReason, NativePath)>,
}
impl From<ErrorCode> for Failure {
    fn from(error: ErrorCode) -> Self {
        Self {
            error,
            problem: None,
        }
    }
}

#[cfg(unix)]
fn units(name: &OsStr) -> Vec<u16> {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes()
        .iter()
        .map(|byte| u16::from(*byte))
        .collect()
}
#[cfg(windows)]
fn units(name: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    name.encode_wide().collect()
}
#[cfg(unix)]
fn native(name: &[u16]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(name.iter().map(|unit| *unit as u8).collect())
}
#[cfg(windows)]
fn native(name: &[u16]) -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(name)
}
fn equal_name(left: &[u16], right: &[u16]) -> bool {
    #[cfg(unix)]
    {
        left == right
    }
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn CompareStringOrdinal(
                a: *const u16,
                na: i32,
                b: *const u16,
                nb: i32,
                ignore_case: i32,
            ) -> i32;
        }
        if left.len() > 32768 || right.len() > 32768 {
            return false;
        }
        // Windows의 파일명 비교와 같은 대소문자 규칙을 사용함.
        unsafe {
            CompareStringOrdinal(
                left.as_ptr(),
                left.len() as i32,
                right.as_ptr(),
                right.len() as i32,
                1,
            ) == 2
        }
    }
}
fn number(suffix: &[u16]) -> Option<u32> {
    if suffix.is_empty() || suffix.iter().any(|unit| !(48..=57).contains(unit)) {
        return None;
    }
    Some(suffix.iter().fold(0u32, |value, digit| {
        value
            .saturating_mul(10)
            .saturating_add(u32::from(*digit - 48))
    }))
}
struct Name {
    prefix: Vec<u16>,
    selected: u32,
    width: usize,
}
impl Name {
    fn parse(path: &Path) -> Option<Self> {
        let name = units(path.file_name()?);
        let dot = name.iter().rposition(|unit| *unit == 46)?;
        let prefix = &name[..=dot];
        let suffix = &name[dot + 1..];
        if prefix.len() < 4 || !matches!(&prefix[prefix.len() - 4..], [46, 55, 90 | 122, 46]) {
            return None;
        }
        Some(Self {
            prefix: prefix.to_vec(),
            selected: number(suffix)?,
            width: suffix.len(),
        })
    }
    fn volume(&self, number: usize) -> NativePath {
        let mut name = self.prefix.clone();
        name.extend(format!("{number:03}").bytes().map(u16::from));
        NativePath::from_path(Path::new(&native(&name)))
    }
    fn problem(&self, reason: VolumeProblemReason, index: usize) -> Failure {
        Failure {
            error: reason.terminal_error(),
            problem: Some((reason, self.volume(index))),
        }
    }
    fn incomplete(&self) -> Failure {
        self.problem(VolumeProblemReason::IncompleteSet, 1)
    }
}
pub(crate) fn is_split(path: &Path) -> bool {
    Name::parse(path).is_some()
}
pub(crate) fn unsupported_split(path: &Path) -> bool {
    let Some(leaf) = path.file_name() else {
        return false;
    };
    let name = units(leaf);
    let Some(dot) = name.iter().rposition(|unit| *unit == 46) else {
        return false;
    };
    let suffix = &name[dot + 1..];
    let numeric = number(suffix).is_some();
    (numeric && dot >= 4 && matches!(&name[dot - 4..dot], [46, 90 | 122, 73 | 105, 80 | 112]))
        || (suffix.len() == 3 && matches!(suffix[0], 90 | 122) && number(&suffix[1..]).is_some())
}

struct Part {
    name: OsString,
    object: SourceObject,
    length: u64,
    modified: SystemTime,
    hash: [u8; 32],
}
pub(crate) struct Group {
    parent: Input,
    path: PathBuf,
    object: SourceObject,
    name: Name,
    parts: Vec<Part>,
    total: u64,
    metadata_limits: crate::archive::MetadataLimits,
}
pub(crate) struct Captured {
    pub snapshot: Snapshot,
    pub group: Group,
}
fn metadata(file: &File) -> std::result::Result<Metadata, ErrorCode> {
    let metadata = file.metadata().map_err(crate::space::io_error)?;
    if !metadata.is_file() {
        return Err(ErrorCode::UnsafePath);
    }
    Ok(metadata)
}
fn combined_length(total: u64, next: u64) -> Result<u64> {
    total
        .checked_add(next)
        .ok_or_else(|| ErrorCode::Limit.into())
}
impl Group {
    fn scan(
        &self,
        step: &mut impl FnMut(Step) -> std::result::Result<(), ErrorCode>,
    ) -> Result<Vec<OsString>> {
        let mut names: Vec<Option<OsString>> = (0..MAX_VOLUMES).map(|_| None).collect();
        let mut metadata_bytes = 0usize;
        step(Step::Check)?;
        let entries = self.parent.children(&self.path)?;
        step(Step::Activity)?;
        for entry in entries {
            step(Step::Check)?;
            let leaf = entry.map_err(crate::space::io_error)?.file_name();
            let value = units(&leaf);
            if value.len() > self.name.prefix.len()
                && equal_name(&value[..self.name.prefix.len()], &self.name.prefix)
            {
                let suffix = &value[self.name.prefix.len()..];
                if let Some(index) = number(suffix) {
                    if index > MAX_VOLUMES as u32 {
                        return Err(ErrorCode::Limit.into());
                    }
                    if index == 0 || suffix.len() != 3 {
                        return Err(self.name.incomplete());
                    }
                    metadata_bytes = value
                        .len()
                        .checked_mul(8)
                        .and_then(|cost| cost.checked_add(256))
                        .and_then(|cost| metadata_bytes.checked_add(cost))
                        .filter(|cost| *cost <= self.metadata_limits.bytes)
                        .ok_or(ErrorCode::Limit)?;
                    if index as usize > self.metadata_limits.nodes {
                        return Err(ErrorCode::Limit.into());
                    }
                    let slot = &mut names[index as usize - 1];
                    if slot.is_some() {
                        return Err(self.name.incomplete());
                    }
                    *slot = Some(leaf);
                }
            }
            step(Step::Activity)?;
        }
        let Some(last) = names.iter().rposition(Option::is_some) else {
            return Ok(Vec::new());
        };
        names.truncate(last + 1);
        names
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                name.ok_or_else(|| {
                    self.name
                        .problem(VolumeProblemReason::MissingVolume, index + 1)
                })
            })
            .collect()
    }
    fn parent_unchanged(&self) -> Result<()> {
        let named = Input::open(&self.path).map_err(|_| ErrorCode::SourceChanged)?;
        if filesystem::object(&named.file)? != self.object
            || !named
                .file
                .metadata()
                .map_err(crate::space::io_error)?
                .is_dir()
        {
            return Err(ErrorCode::SourceChanged.into());
        }
        Ok(())
    }
    fn open_part(
        &self,
        name: &OsStr,
        step: &mut impl FnMut(Step) -> std::result::Result<(), ErrorCode>,
    ) -> Result<Input> {
        match self.parent.open_named(&self.path, name) {
            Ok(file) => {
                step(Step::Activity)?;
                Ok(file)
            }
            Err(error) => {
                // 읽기 실패가 확정된 중간 누락인지 먼저 확인함.
                if error == ErrorCode::Io {
                    let names = self.scan(step)?;
                    if !names.iter().any(|candidate| candidate == name) {
                        return Err(ErrorCode::SourceChanged.into());
                    }
                }
                Err(error.into())
            }
        }
    }
    fn matches(&self, part: &Part, input: &Input) -> Result<()> {
        let data = metadata(&input.file)?;
        if filesystem::object(&input.file)? != part.object
            || data.len() != part.length
            || data.modified().map_err(crate::space::io_error)? != part.modified
        {
            return Err(ErrorCode::SourceChanged.into());
        }
        Ok(())
    }
    fn read_part(
        &self,
        part: &Part,
        output: &mut Option<&mut File>,
        whole: &mut Sha256,
        processed: &mut u64,
        reported: &mut Instant,
        step: &mut impl FnMut(Step) -> std::result::Result<(), ErrorCode>,
    ) -> Result<[u8; 32]> {
        let mut input = self.open_part(&part.name, step)?;
        self.matches(part, &input)?;
        let mut hash = Sha256::new();
        let mut read = 0u64;
        let mut buffer = zeroize::Zeroizing::new([0; 65536]);
        loop {
            step(Step::Check)?;
            let length = input
                .file
                .read(&mut buffer[..])
                .map_err(crate::space::io_error)?;
            step(Step::Activity)?;
            if length == 0 {
                break;
            }
            read = read
                .checked_add(length as u64)
                .filter(|n| *n <= part.length)
                .ok_or(ErrorCode::SourceChanged)?;
            if let Some(file) = output {
                file.write_all(&buffer[..length])
                    .map_err(crate::space::io_error)?;
                step(Step::Activity)?;
            }
            hash.update(&buffer[..length]);
            whole.update(&buffer[..length]);
            *processed = processed
                .checked_add(length as u64)
                .ok_or(ErrorCode::Limit)?;
            if reported.elapsed() >= std::time::Duration::from_millis(200) {
                step(Step::Read {
                    bytes: *processed,
                    total: self.total,
                })?;
                *reported = Instant::now();
            }
        }
        if read != part.length {
            return Err(ErrorCode::SourceChanged.into());
        }
        self.matches(part, &input)?;
        let named = self.open_part(&part.name, step)?;
        self.matches(part, &named)?;
        Ok(hash.finalize().into())
    }
    fn manifest(&self) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"zipelo.split7z.v1\0");
        hash.update(
            crate::wire::encode_source_object(&self.object).map_err(|_| ErrorCode::Internal)?,
        );
        hash.update((self.parts.len() as u16).to_le_bytes());
        for (index, part) in self.parts.iter().enumerate() {
            hash.update(((index + 1) as u16).to_le_bytes());
            hash.update(
                crate::wire::encode_native_path(&NativePath::from_path(Path::new(&part.name)))
                    .map_err(|_| ErrorCode::Limit)?,
            );
            hash.update(
                crate::wire::encode_source_object(&part.object).map_err(|_| ErrorCode::Internal)?,
            );
            hash.update(part.length.to_le_bytes());
            hash.update(part.hash);
        }
        Ok(hash.finalize().into())
    }
    pub fn verify(
        &self,
        step: &mut impl FnMut(Step) -> std::result::Result<(), ErrorCode>,
    ) -> Result<()> {
        step(Step::Check)?;
        self.parent_unchanged()?;
        let names = self.scan(step).map_err(|failure| {
            if matches!(
                failure.problem,
                Some((VolumeProblemReason::IncompleteSet, _))
            ) {
                ErrorCode::SourceChanged.into()
            } else {
                failure
            }
        })?;
        if names.len() != self.parts.len()
            || names
                .iter()
                .zip(&self.parts)
                .any(|(name, part)| *name != part.name)
        {
            return Err(ErrorCode::SourceChanged.into());
        }
        let mut processed = 0;
        let mut whole = Sha256::new();
        let mut reported = Instant::now();
        for part in &self.parts {
            step(Step::Check)?;
            if self.read_part(
                part,
                &mut None,
                &mut whole,
                &mut processed,
                &mut reported,
                step,
            )? != part.hash
            {
                return Err(ErrorCode::SourceChanged.into());
            }
            step(Step::Activity)?;
        }
        step(Step::Read {
            bytes: processed,
            total: self.total,
        })?;
        let final_names = self.scan(step).map_err(|failure| {
            if matches!(
                failure.problem,
                Some((VolumeProblemReason::IncompleteSet, _))
            ) {
                ErrorCode::SourceChanged.into()
            } else {
                failure
            }
        })?;
        if final_names != names {
            return Err(ErrorCode::SourceChanged.into());
        }
        self.parent_unchanged()?;
        Ok(())
    }
}
pub(crate) fn open(
    path: &Path,
    expected: Option<&ListId>,
    limit: u64,
    metadata_limits: crate::archive::MetadataLimits,
    step: &mut impl FnMut(Step) -> std::result::Result<(), ErrorCode>,
) -> Result<Option<Captured>> {
    if unsupported_split(path) {
        return Err(ErrorCode::UnsupportedFeature.into());
    }
    let Some(name) = Name::parse(path) else {
        return Ok(None);
    };
    if !NativePath::from_path(Path::new(
        path.file_name().ok_or(ErrorCode::InvalidRequest)?,
    ))
    .is_basename()
    {
        return Err(ErrorCode::UnsafePath.into());
    }
    if name.selected > MAX_VOLUMES as u32 {
        return Err(ErrorCode::Limit.into());
    }
    if name.width != 3 {
        return Err(ErrorCode::UnsupportedFeature.into());
    }
    if name.selected != 1 {
        return Err(name.problem(VolumeProblemReason::FirstVolumeRequired, 1));
    }
    step(Step::Check)?;
    let parent_path = path
        .parent()
        .ok_or(ErrorCode::InvalidRequest)?
        .to_path_buf();
    let parent = Input::open(&parent_path)?;
    if !parent
        .file
        .metadata()
        .map_err(crate::space::io_error)?
        .is_dir()
    {
        return Err(ErrorCode::UnsafePath.into());
    }
    let object = filesystem::object(&parent.file)?;
    let mut group = Group {
        parent,
        path: parent_path,
        object,
        name,
        parts: Vec::new(),
        total: 0,
        metadata_limits,
    };
    let names = group.scan(step)?;
    if names.is_empty() {
        return Err(if expected.is_some() {
            ErrorCode::SourceChanged
        } else {
            ErrorCode::Io
        }
        .into());
    }
    for name in names {
        step(Step::Check)?;
        let input = group.open_part(&name, step)?;
        let data = metadata(&input.file)?;
        let object = filesystem::object(&input.file)?;
        if group.parts.iter().any(|part| part.object == object) {
            return Err(ErrorCode::UnsafePath.into());
        }
        group.total = combined_length(group.total, data.len())?;
        group.parts.push(Part {
            name,
            object,
            length: data.len(),
            modified: data.modified().map_err(crate::space::io_error)?,
            hash: [0; 32],
        });
        step(Step::Activity)?;
    }
    if expected.is_some_and(|id| {
        id.archive_bytes != group.total
            || id.catalog_version != 2
            || id.format != Format::SevenZip
            || !matches!(id.source_object, SourceObject::SplitSevenZip { .. })
    }) {
        return Err(ErrorCode::SourceChanged.into());
    }
    if group.total > limit {
        return Err(ErrorCode::Limit.into());
    }
    let mut file = crate::spool::temporary()?;
    crate::space::ensure(&file, group.total)?;
    let mut processed = 0;
    let mut whole = Sha256::new();
    let mut reported = Instant::now();
    for index in 0..group.parts.len() {
        let hash = group.read_part(
            &group.parts[index],
            &mut Some(&mut file),
            &mut whole,
            &mut processed,
            &mut reported,
            step,
        )?;
        group.parts[index].hash = hash;
        step(Step::Activity)?;
    }
    step(Step::Read {
        bytes: processed,
        total: group.total,
    })?;
    let hash = whole.finalize().into();
    let object = SourceObject::SplitSevenZip {
        manifest_sha256: group.manifest()?,
    };
    if expected.is_some_and(|id| id.source_object != object || id.archive_sha256 != hash) {
        return Err(ErrorCode::SourceChanged.into());
    }
    // 마지막 누락 이름을 추측하지 않고 논리 헤더 끝 위치를 검증함.
    file.rewind().map_err(crate::space::io_error)?;
    let mut header = [0; 32];
    if group.parts.iter().any(|part| part.length == 0) || group.total < 32 {
        return Err(group.name.incomplete());
    }
    file.read_exact(&mut header)
        .map_err(crate::space::io_error)?;
    if &header[..6] != b"7z\xbc\xaf\x27\x1c"
        || zlib_rs::crc32::crc32(0, &header[12..])
            != u32::from_le_bytes(header[8..12].try_into().unwrap())
    {
        return Err(group.name.incomplete());
    }
    let offset = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let length = u64::from_le_bytes(header[20..28].try_into().unwrap());
    if 32u64
        .checked_add(offset)
        .and_then(|start| start.checked_add(length))
        != Some(group.total)
    {
        return Err(group.name.incomplete());
    }
    group.verify(step)?;
    file.rewind().map_err(crate::space::io_error)?;
    Ok(Some(Captured {
        snapshot: Snapshot {
            file,
            len: group.total,
            object,
            hash,
        },
        group,
    }))
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/split_capture.rs"]
mod tests;
