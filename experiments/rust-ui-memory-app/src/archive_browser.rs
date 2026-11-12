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
