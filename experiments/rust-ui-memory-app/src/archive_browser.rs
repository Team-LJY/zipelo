use crate::archive_ui::{Entry, Kind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Row {
    pub path: String,
    pub name: String,
    pub directory: bool,
    pub ids: BTreeSet<u32>,
    pub bytes: Option<u64>,
    pub encrypted: bool,
}

pub fn normalized(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    let name = name.trim_end_matches('/');
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(':')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\0'))
    {
        return None;
    }
    Some(name.to_owned())
}

pub fn rows(entries: &[Entry], current: &str, query: &str) -> Vec<Row> {
    let prefix = if current.is_empty() {
        String::new()
    } else {
        format!("{current}/")
    };
    let mut rows = BTreeMap::<String, Row>::new();
    for entry in entries {
        let Some(path) = normalized(&entry.name) else {
            continue;
        };
        let Some(relative) = path.strip_prefix(&prefix) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let (name, implicit) = relative
            .split_once('/')
            .map_or((relative, false), |(first, _)| (first, true));
        let directory = implicit || entry.kind == Kind::Directory;
        let row = rows.entry(name.to_owned()).or_insert_with(|| Row {
            path: format!("{prefix}{name}"),
            name: name.to_owned(),
            directory,
            ids: BTreeSet::new(),
            bytes: None,
            encrypted: false,
        });
        row.directory |= directory;
        if matches!(entry.kind, Kind::File | Kind::Directory) {
            row.ids.insert(entry.id);
        }
        if !directory {
            row.bytes = entry.bytes;
        } else if entry.kind == Kind::File {
            // 폴더 크기는 안에 있는 모든 파일 크기의 합으로 표시함.
            let total = row.bytes.unwrap_or(0);
            row.bytes = Some(total.saturating_add(entry.bytes.unwrap_or(0)));
        } else if row.bytes.is_none() {
            row.bytes = Some(0);
        }
        row.encrypted |= entry.encrypted;
    }
    let query = query.to_lowercase();
    let mut rows = rows
        .into_values()
        .filter(|row| row.name.to_lowercase().contains(&query))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

pub fn expand_selection(entries: &[Entry], selected: &BTreeSet<u32>) -> BTreeSet<u32> {
    let directories = entries
        .iter()
        .filter(|entry| entry.kind == Kind::Directory && selected.contains(&entry.id))
        .filter_map(|entry| normalized(&entry.name).map(|path| format!("{path}/")))
        .collect::<Vec<_>>();
    entries
        .iter()
        .filter(|entry| {
            selected.contains(&entry.id)
                || normalized(&entry.name)
                    .is_some_and(|name| directories.iter().any(|prefix| name.starts_with(prefix)))
        })
        .filter(|entry| matches!(entry.kind, Kind::File | Kind::Directory))
        .map(|entry| entry.id)
        .collect()
}

pub fn remove_selection(entries: &[Entry], selected: &mut BTreeSet<u32>, removed: &BTreeSet<u32>) {
    let paths = entries
        .iter()
        .filter(|entry| removed.contains(&entry.id))
        .filter_map(|entry| normalized(&entry.name))
        .collect::<Vec<_>>();
    let ancestors = entries
        .iter()
        .filter(|entry| entry.kind == Kind::Directory)
        .filter_map(|entry| {
            let prefix = format!("{}/", normalized(&entry.name)?);
            paths
                .iter()
                .any(|path| path.starts_with(&prefix))
                .then_some(entry.id)
        })
        .collect::<BTreeSet<_>>();
    selected.retain(|id| !removed.contains(id) && !ancestors.contains(id));
}

/// 찾기 창의 결과 한 줄.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Found {
    /// 압축파일 안 전체 경로 (앞뒤 '/' 없음).
    pub path: String,
    pub name: String,
    pub directory: bool,
}

/// 찾기 결과를 너무 많이 그리지 않도록 정한 최대 개수.
pub const FIND_LIMIT: usize = 5000;

/// 압축파일 안에서 검색어가 들어간 파일과 폴더를 찾음. 대소문자는 구분하지 않음.
///
/// - `whole_path`가 false면(Ctrl+F) `base` 폴더 아래의 모든 하위 폴더를 찾아 내려가며
///   이름에 검색어가 들어간 항목을 찾음.
/// - `whole_path`가 true면(Ctrl+Shift+F) 압축파일 전체에서 전체 경로에 검색어가 들어간
///   항목을 찾음. 이때 `base`는 쓰지 않음.
///
/// 압축파일에 따로 적혀 있지 않은 상위 폴더도 결과에 포함함. 결과는 경로 순서이며
/// `limit`개를 넘으면 잘라 내고 두 번째 값으로 true를 돌려줌.
pub fn find(
    entries: &[Entry],
    base: &str,
    query: &str,
    whole_path: bool,
    limit: usize,
) -> (Vec<Found>, bool) {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (Vec::new(), false);
    }
    let base = base.trim_matches('/');
    let prefix = if whole_path || base.is_empty() {
        String::new()
    } else {
        format!("{base}/")
    };
    let mut paths = BTreeMap::<String, bool>::new();
    for entry in entries {
        if !matches!(entry.kind, Kind::File | Kind::Directory) {
            continue;
        }
        let Some(path) = normalized(&entry.name) else {
            continue;
        };
        if !path.starts_with(&prefix) {
            continue;
        }
        // 상위 폴더는 압축파일에 따로 없어도 폴더로 넣음.
        let mut end = prefix.len();
        while let Some(slash) = path[end..].find('/') {
            end += slash;
            paths.insert(path[..end].to_owned(), true);
            end += 1;
        }
        let directory = entry.kind == Kind::Directory;
        *paths.entry(path).or_insert(directory) |= directory;
    }
    let mut found = Vec::new();
    for (path, directory) in paths {
        let name = path.rsplit('/').next().unwrap_or(&path);
        let haystack = if whole_path { path.as_str() } else { name };
        if !haystack.to_lowercase().contains(&query) {
            continue;
        }
        if found.len() == limit {
            return (found, true);
        }
        found.push(Found {
            name: name.to_owned(),
            path,
            directory,
        });
    }
    (found, false)
}
