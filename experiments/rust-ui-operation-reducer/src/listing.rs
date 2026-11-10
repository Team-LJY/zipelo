use crate::{Disposition, Model, Rejection, UiOperationId};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: RowId,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowKey {
    pub operation: UiOperationId,
    pub generation: u64,
    pub row: RowId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputArea {
    List,
    Text,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigation {
    Next,
    Previous,
    First,
    Last,
    PageUp(usize),
    PageDown(usize),
}

impl Model {
    pub(crate) fn replace_list(&mut self, operation: UiOperationId, rows: Vec<Row>) -> Disposition {
        if self.operation.as_ref().map(|op| op.mapping.ui) != Some(operation) {
            return Disposition::Ignored(Rejection::StaleOperation);
        }
        let mut ids = HashSet::new();
        if rows.len() > 100_000
            || rows
                .iter()
                .any(|row| row.id.0 == 0 || row.name.len() > 4096 || !ids.insert(row.id))
        {
            return Disposition::Ignored(Rejection::InvalidList);
        }
        let Some(generation) = self.generation.checked_add(1) else {
            return Disposition::Ignored(Rejection::GenerationOverflow);
        };
        self.generation = generation;
        self.rows = rows;
        self.selected = None;
        Disposition::Applied
    }

    pub(crate) fn navigate(&mut self, key: Navigation) {
        if self.rows.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|key| self.rows.iter().position(|r| r.id == key.row));
        let index = current.unwrap_or(0);
        let last = self.rows.len() - 1;
        let index = match key {
            Navigation::First => 0,
            Navigation::Last => last,
            Navigation::Next if current.is_none() => 0,
            Navigation::Next => index.saturating_add(1).min(last),
            Navigation::Previous => index.saturating_sub(1),
            Navigation::PageUp(n) => index.saturating_sub(n),
            Navigation::PageDown(n) => index.saturating_add(n).min(last),
        };
        self.selected = self.row_key(self.rows[index].id);
    }
}
