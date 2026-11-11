use crate::{gzip, publication, snapshot};
use std::{ffi::OsStr, path::Path};

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub snapshot: snapshot::Limits,
    pub gzip: gzip::Limits,
    pub publication: publication::Limits,
}

pub struct Request<'a> {
    pub input: &'a Path,
    pub output_parent: &'a Path,
    pub output_leaf: &'a OsStr,
    pub limits: Limits,
}

// 서로 다른 단위와 원래 누적 카운터를 합산하거나 환불하지 않음.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    pub snapshot: snapshot::Usage,
    pub gzip: gzip::Usage,
    pub publication: publication::Usage,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Snapshot(snapshot::Checkpoint),
    Decode(gzip::Checkpoint),
    Publication(publication::Checkpoint),
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    pub event: Event,
    pub usage: Usage,
}

#[derive(Debug)]
pub enum Stop<E> {
    Cancelled,
    Callback(E),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterFailure {
    MissingSelectedOutput,
    SelectedLengthMismatch { expected: u64, actual: u64 },
}

// 원래 오류와 파일 계층의 게시/내구성/close 보고서를 그대로 보존함.
pub enum Outcome<E> {
    SnapshotFailed(snapshot::Failure<E>),
    DecodeFailed(gzip::Error<E>),
    AdapterFailed(AdapterFailure),
    Publication(publication::Report<E>),
}

#[derive(Clone, Copy, Debug)]
pub struct SnapshotObservation {
    identity: snapshot::Identity,
    sha256: [u8; 32],
}

impl SnapshotObservation {
    pub(crate) fn from_capture(value: &snapshot::FileSnapshot) -> Self {
        Self {
            identity: value.identity(),
            sha256: value.sha256(),
        }
    }

    pub fn identity(&self) -> snapshot::Identity {
        self.identity
    }

    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GzipObservation {
    boundary: gzip::Boundary,
}

impl GzipObservation {
    pub(crate) fn from_decode(boundary: gzip::Boundary) -> Self {
        Self { boundary }
    }

    pub fn boundary(&self) -> gzip::Boundary {
        self.boundary
    }
}

pub struct Report<E> {
    pub outcome: Outcome<E>,
    pub usage: Usage,
    pub snapshot: Option<SnapshotObservation>,
    pub decoded: Option<GzipObservation>,
}
