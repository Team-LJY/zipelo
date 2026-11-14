use crate::{
    archive::{self, Member, MetadataBudget},
    filesystem,
    protocol::*,
};
use std::{
    fs::{self, File, Metadata},
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

type Result<T> = std::result::Result<T, ErrorCode>;

pub(super) struct Input {
    pub file: File,
    #[cfg(windows)]
    _parents: Vec<File>,
}
impl Input {
    #[cfg(target_os = "linux")]
    fn child(file: &File, name: &std::ffi::OsStr) -> Result<File> {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        };
        unsafe extern "C" {
            fn openat(fd: i32, name: *const i8, flags: i32, ...) -> i32;
        }
        let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| ErrorCode::UnsafePath)?;
        // 링크를 따라가지 않는 경로 핸들로 객체 종류를 확인함.
        let fd = unsafe {
            openat(
                file.as_raw_fd(),
                name.as_ptr(),
                0x80000 | 0x20000 | 0x200000,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return Err(if matches!(error.raw_os_error(), Some(20 | 40)) {
                ErrorCode::UnsafePath
            } else {
                crate::space::io_error(error)
            });
        }
        let pinned = unsafe { File::from_raw_fd(fd) };
        Self::validate(&pinned)?;
        Ok(pinned)
    }
    #[cfg(target_os = "linux")]
    fn read(pinned: File) -> Result<File> {
        use std::os::fd::AsRawFd;
        // 읽기 권한은 최종 입력에만 요청함.
        File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd())).map_err(crate::space::io_error)
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(ErrorCode::InvalidRequest);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(0x200000 | 0x20000 | 0x10000 | 0x80000)
                .open("/")
                .map_err(crate::space::io_error)?;
            for part in path.components() {
                match part {
                    Component::RootDir => {}
                    Component::Normal(name) => file = Self::child(&file, name)?,
                    _ => return Err(ErrorCode::UnsafePath),
                }
            }
            let file = Self::read(file)?;
            Self::validate(&file)?;
            Ok(Self { file })
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let mut current = PathBuf::new();
            let mut parents = Vec::new();
            for part in path.components() {
                match part {
                    Component::Prefix(_) | Component::RootDir => current.push(part.as_os_str()),
                    Component::Normal(name) => {
                        if parents.is_empty() {
                            parents.push(Self::lock(&current)?);
                        }
                        current.push(name);
                        parents.push(Self::lock(&current)?);
                    }
                    _ => return Err(ErrorCode::UnsafePath),
                }
            }
            let file = fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .custom_flags(0x02000000 | 0x00200000)
                .open(&current)
                .map_err(crate::space::io_error)?;
            Self::validate(&file)?;
            Ok(Self {
                file,
                _parents: parents,
            })
        }
    }
    #[cfg(windows)]
    fn lock(path: &Path) -> Result<File> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let file = fs::OpenOptions::new()
            .access_mode(0)
            .share_mode(3)
            .custom_flags(0x02000000 | 0x00200000)
            .open(path)
            .map_err(crate::space::io_error)?;
        if file
            .metadata()
            .map_err(crate::space::io_error)?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err(ErrorCode::UnsafePath);
        }
        Ok(file)
    }
    fn validate(file: &File) -> Result<()> {
        let metadata = file.metadata().map_err(crate::space::io_error)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(ErrorCode::UnsafePath);
            }
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(ErrorCode::UnsafePath);
        }
        Ok(())
    }
    fn open_child(&self, path: &Path, name: &str) -> Result<Self> {
        self.open_named(path, std::ffi::OsStr::new(name))
    }
    pub(crate) fn open_named(&self, path: &Path, name: &std::ffi::OsStr) -> Result<Self> {
        if !NativePath::from_path(Path::new(name)).is_basename() {
            return Err(ErrorCode::UnsafePath);
        }
        #[cfg(target_os = "linux")]
        {
            let _ = path;
            let file = Self::read(Self::child(&self.file, name)?)?;
            Self::validate(&file)?;
            Ok(Self { file })
        }
        #[cfg(windows)]
        {
            Self::open(&path.join(name))
        }
    }
    pub(crate) fn children(&self, path: &Path) -> Result<fs::ReadDir> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let _ = path;
            fs::read_dir(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
                .map_err(crate::space::io_error)
        }
        #[cfg(windows)]
        {
            fs::read_dir(path).map_err(crate::space::io_error)
        }
    }
}

pub(super) struct Planned {
    pub member: Member,
    path: PathBuf,
    object: SourceObject,
    modified: Option<SystemTime>,
}
impl Planned {
    pub fn open(&self) -> Result<Input> {
        let input = Input::open(&self.path).map_err(|_| ErrorCode::SourceChanged)?;
        if !self.matches(&input.file)? {
            return Err(ErrorCode::SourceChanged);
        }
        Ok(input)
    }
    fn matches(&self, file: &File) -> Result<bool> {
        let metadata = file.metadata().map_err(crate::space::io_error)?;
        Ok(filesystem::object(file)? == self.object
            && metadata.modified().ok() == self.modified
            && metadata.is_dir() == (self.member.entry.kind == EntryKind::Directory)
            && (metadata.is_dir() || metadata.len() == self.member.entry.size))
    }
}
pub(super) struct Plan {
    pub items: Vec<Planned>,
    limits: Limits,
    budget: MetadataBudget,
    total: u64,
}
impl Plan {
    pub fn new(
        sources: &[Source],
        limits: Limits,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<Self> {
        if sources.len() > limits.entries as usize {
            return Err(ErrorCode::Limit);
        }
        let mut plan = Self {
            items: Vec::new(),
            limits,
            budget: MetadataBudget::default(),
            total: 0,
        };
        let mut roots = std::collections::BTreeSet::new();
        for source in sources {
            check.check()?;
            let name = archive::valid_name(&source.archive_name, false)?;
            if !roots.insert(name.to_lowercase()) {
                return Err(ErrorCode::Conflict);
            }
            crate::resources::completed(Ok(()), check)?;
        }
        for source in sources {
            let path = source.path.to_path()?;
            let input = crate::resources::completed(Input::open(&path), check)?;
            plan.visit(path, source.archive_name.clone(), input, check)?;
        }
        let members = plan
            .items
            .iter()
            .map(|p| {
                crate::resources::completed(
                    Ok(Member {
                        entry: p.member.entry.clone(),
                        data: Vec::new(),
                    }),
                    check,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        archive::validate_members_with(&members, check)?;
        Ok(plan)
    }
    fn visit(
        &mut self,
        path: PathBuf,
        name: String,
        input: Input,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<()> {
        check.check()?;
        if self.items.len() >= self.limits.entries as usize {
            return Err(ErrorCode::Limit);
        }
        let metadata: Metadata = crate::resources::completed(
            input.file.metadata().map_err(crate::space::io_error),
            check,
        )?;
        let directory = metadata.is_dir();
        let name = archive::valid_name(&name, directory)?;
        self.budget.charge_with(&name, check.metadata_limits())?;
        self.budget
            .charge_copy(&path.to_string_lossy(), check.metadata_limits())?;
        let size = if directory { 0 } else { metadata.len() };
        self.total = self
            .total
            .checked_add(size)
            .filter(|n| *n <= self.limits.input.min(self.limits.output))
            .ok_or(ErrorCode::Limit)?;
        check.observe(
            ResourceUsage {
                estimated_bytes: Some(self.total),
                actual_entries: Some(self.items.len() as u32 + 1),
                ..ResourceUsage::default()
            },
            None,
        )?;
        crate::resources::completed(Ok(()), check)?;
        self.items.push(Planned {
            member: Member {
                entry: Entry {
                    entry_id: self.items.len() as u32,
                    name: name.clone(),
                    kind: if directory {
                        EntryKind::Directory
                    } else {
                        EntryKind::File
                    },
                    size,
                    compressed_size: 0,
                    encrypted: false,
                },
                data: Vec::new(),
            },
            path: path.clone(),
            object: crate::resources::completed(filesystem::object(&input.file), check)?,
            modified: metadata.modified().ok(),
        });
        if directory {
            let mut children = Vec::new();
            for entry in input.children(&path)? {
                check.check()?;
                if self.items.len() + children.len() >= self.limits.entries as usize {
                    return Err(ErrorCode::Limit);
                }
                let leaf = entry
                    .map_err(crate::space::io_error)?
                    .file_name()
                    .into_string()
                    .map_err(|_| ErrorCode::UnsafePath)?;
                self.budget.charge_copy(&leaf, check.metadata_limits())?;
                children.push(leaf);
                check.observe(
                    ResourceUsage {
                        estimated_bytes: Some(self.total),
                        estimated_entries: Some((self.items.len() + children.len()) as u32),
                        ..ResourceUsage::default()
                    },
                    None,
                )?;
                crate::resources::completed(Ok(()), check)?;
            }
            children.sort();
            for leaf in children {
                let child = crate::resources::completed(input.open_child(&path, &leaf), check)?;
                self.visit(path.join(&leaf), format!("{name}/{leaf}"), child, check)?;
            }
        }
        Ok(())
    }
    pub fn reserve_metadata(
        &mut self,
        bytes: usize,
        check: &impl crate::resources::Monitor,
    ) -> Result<()> {
        self.budget.charge_bytes(bytes, check.metadata_limits())
    }
    pub fn verify(&self, check: &mut impl crate::resources::Monitor) -> Result<()> {
        for item in &self.items {
            check.check()?;
            item.open()?;
            crate::resources::completed(Ok(()), check)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/create_sources.rs"]
mod tests;
