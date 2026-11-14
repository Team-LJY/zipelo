use crate::protocol::*;
use std::io::{self, Read, Write};

pub const MAX_BODY: usize = 1_048_576;
pub const MAX_FRAMES: u64 = 1_000_000;
const LEGACY_VECTOR_ITEMS: usize = 32_768;
const MAX_SELECTION_IDS: usize = 200_000;
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid worker frame")
}
struct Encoder {
    bytes: Vec<u8>,
    selection_limit: usize,
}
impl Encoder {
    fn put(&mut self, b: &[u8]) -> io::Result<()> {
        if self
            .bytes
            .len()
            .checked_add(b.len())
            .filter(|n| *n <= MAX_BODY)
            .is_none()
        {
            return Err(invalid());
        }
        self.bytes.extend_from_slice(b);
        Ok(())
    }
}
struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
    selection_limit: usize,
}
impl<'a> Decoder<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(invalid)?;
        let b = self.bytes.get(self.at..end).ok_or_else(invalid)?;
        self.at = end;
        Ok(b)
    }
}
trait Wire: Sized {
    fn enc(&self, e: &mut Encoder) -> io::Result<()>;
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self>;
}
macro_rules! integer { ($($t:ty),*) => { $(impl Wire for $t {
    fn enc(&self,e:&mut Encoder)->io::Result<()> { e.put(&self.to_le_bytes()) }
    fn dec(d:&mut Decoder<'_>)->io::Result<Self> { Ok(Self::from_le_bytes(d.take(size_of::<Self>())?.try_into().map_err(|_|invalid())?)) }
})* }; }
integer!(u8, u16, u32, u64);
impl<const N: usize> Wire for [u8; N] {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        e.put(self)
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        d.take(N)?.try_into().map_err(|_| invalid())
    }
}
impl Wire for bool {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        u8::from(*self).enc(e)
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        match u8::dec(d)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid()),
        }
    }
}
impl<T: Wire> Wire for Option<T> {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        self.is_some().enc(e)?;
        if let Some(v) = self {
            v.enc(e)?;
        }
        Ok(())
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        if bool::dec(d)? {
            Ok(Some(T::dec(d)?))
        } else {
            Ok(None)
        }
    }
}
fn encode_vec<T: Wire>(values: &[T], limit: usize, e: &mut Encoder) -> io::Result<()> {
    if values.len() > limit {
        return Err(invalid());
    }
    (values.len() as u32).enc(e)?;
    for value in values {
        value.enc(e)?;
    }
    Ok(())
}
fn decode_vec<T: Wire>(limit: usize, d: &mut Decoder<'_>) -> io::Result<Vec<T>> {
    let n = u32::dec(d)? as usize;
    if n > limit || n > d.bytes.len() - d.at {
        return Err(invalid());
    }
    let mut values = Vec::new();
    values.try_reserve(n).map_err(|_| invalid())?;
    for _ in 0..n {
        values.push(T::dec(d)?);
    }
    Ok(values)
}
impl<T: Wire> Wire for Vec<T> {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        encode_vec(self, LEGACY_VECTOR_ITEMS, e)
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        decode_vec(LEGACY_VECTOR_ITEMS, d)
    }
}
impl Wire for String {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        if self.len() > 32768 {
            return Err(invalid());
        }
        (self.len() as u32).enc(e)?;
        e.put(self.as_bytes())
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        let n = u32::dec(d)? as usize;
        if n > 32768 {
            return Err(invalid());
        }
        String::from_utf8(d.take(n)?.to_vec()).map_err(|_| invalid())
    }
}
macro_rules! enumeration { ($t:ident {$($v:ident=$n:expr),*$(,)?}) => { impl Wire for $t {
    fn enc(&self,e:&mut Encoder)->io::Result<()> { (match self {$(Self::$v=>$n,)*} as u8).enc(e) }
    fn dec(d:&mut Decoder<'_>)->io::Result<Self> { match u8::dec(d)? {$($n=>Ok(Self::$v),)*_=>Err(invalid())} }
} }; }
enumeration!(Format{Zip=0,Tar=1,Gzip=2,TarGzip=3,SevenZip=4});
enumeration!(Method{Store=0,Deflate=1});
enumeration!(SevenZipMethod{Lzma2=0});
enumeration!(Encryption{None=0,Aes256=1});
enumeration!(ActionKind{List=0,Extract=1,CreateZip=2,Preview=3,CreateSevenZip=4,RecoverZip=5});
enumeration!(RecoveryMode{CentralDirectory=0,LocalChain=1});
enumeration!(RecoveryStatus{Verified=0,Lost=1,Unverified=2});
enumeration!(RecoveryReason{None=0,HeaderRebuilt=1,Truncated=2,CrcMismatch=3,DecodeFailure=4,BoundaryUnknown=5,UnsupportedFeature=6,MetadataMissing=7});
enumeration!(PreviewFormat{Utf8Text=0,Rgba8=1});
enumeration!(EntryKind{File=0,Directory=1,Link=2,Other=3});
enumeration!(Stage{Scan=0,Read=1,Decode=2,Write=3,Verify=4,Commit=5});
enumeration!(Disposition{Created=0,PartialRetained=1,Removed=2,CommitUncertain=3});
enumeration!(Outcome{Completed=0,Failed=1,Cancelled=2});
enumeration!(ErrorCode{None=0,Integrity=1,UnsupportedFormat=2,UnsupportedFeature=3,WrongPassword=4,UnsafePath=5,Conflict=6,Limit=7,Io=8,Protocol=9,Internal=10,SourceChanged=11,InvalidRequest=12,DiskFull=13,MissingVolume=14,RecoveryIncomplete=15,RecoveryImpossible=16});
enumeration!(VolumeProblemReason{FirstVolumeRequired=0,MissingVolume=1,IncompleteSet=2});
enumeration!(Publication{None=0,Partial=1,Complete=2,Uncertain=3});
enumeration!(Durability{NotApplicable=0,Synced=1,Unconfirmed=2,Failed=3});
enumeration!(Cleanup{Complete=0,Failed=1,Unconfirmed=2});
enumeration!(CancelStatus{Requested=0,AlreadyRequested=1,TooLate=2,Finished=3});
enumeration!(PasswordPurpose{Decrypt=0,Encrypt=1});
enumeration!(ConflictChoice{Overwrite=0,Skip=1,Rename=2});
macro_rules! structure {($t:ident {$($f:ident),*})=>{impl Wire for $t {
    fn enc(&self,e:&mut Encoder)->io::Result<()> {$(self.$f.enc(e)?;)*Ok(())}
    fn dec(d:&mut Decoder<'_>)->io::Result<Self> {Ok(Self {$($f:Wire::dec(d)?,)*})}
}};}
structure!(Limits {
    input,
    output,
    entries
});
impl Wire for ListId {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        if !self.valid() {
            return Err(invalid());
        }
        self.catalog_version.enc(e)?;
        self.format.enc(e)?;
        self.source_object.enc(e)?;
        self.archive_bytes.enc(e)?;
        self.archive_sha256.enc(e)
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        let id = Self {
            catalog_version: Wire::dec(d)?,
            format: Wire::dec(d)?,
            source_object: Wire::dec(d)?,
            archive_bytes: Wire::dec(d)?,
            archive_sha256: Wire::dec(d)?,
        };
        if !id.valid() {
            return Err(invalid());
        }
        Ok(id)
    }
}
impl Wire for PreviewInfo {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        if !self.valid() {
            return Err(invalid());
        }
        self.entry_id.enc(e)?;
        self.format.enc(e)?;
        self.total_bytes.enc(e)?;
        self.width.enc(e)?;
        self.height.enc(e)
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        let info = Self {
            entry_id: Wire::dec(d)?,
            format: Wire::dec(d)?,
            total_bytes: Wire::dec(d)?,
            width: Wire::dec(d)?,
            height: Wire::dec(d)?,
        };
        if !info.valid() {
            return Err(invalid());
        }
        Ok(info)
    }
}
impl Wire for RecoveryEntry {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        if !self.valid() {
            return Err(invalid());
        }
        self.candidate_id.enc(e)?;
        self.source_offset.enc(e)?;
        self.name.enc(e)?;
        self.kind.enc(e)?;
        self.status.enc(e)?;
        self.reason.enc(e)?;
        self.declared_size.enc(e)?;
        self.verified_size.enc(e)?;
        self.sha256.enc(e)?;
        Ok(())
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        let value = Self {
            candidate_id: Wire::dec(d)?,
            source_offset: Wire::dec(d)?,
            name: Wire::dec(d)?,
            kind: Wire::dec(d)?,
            status: Wire::dec(d)?,
            reason: Wire::dec(d)?,
            declared_size: Wire::dec(d)?,
            verified_size: Wire::dec(d)?,
            sha256: Wire::dec(d)?,
        };
        if !value.valid() {
            return Err(invalid());
        }
        Ok(value)
    }
}
impl Wire for RecoveryEnd {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        if !self.valid() {
            return Err(invalid());
        }
        self.mode.enc(e)?;
        self.source_bytes.enc(e)?;
        self.source_sha256.enc(e)?;
        self.known_entries.enc(e)?;
        self.verified_entries.enc(e)?;
        self.lost_entries.enc(e)?;
        self.unverified_entries.enc(e)?;
        self.unknown_loss.enc(e)?;
        Ok(())
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        let value = Self {
            mode: Wire::dec(d)?,
            source_bytes: Wire::dec(d)?,
            source_sha256: Wire::dec(d)?,
            known_entries: Wire::dec(d)?,
            verified_entries: Wire::dec(d)?,
            lost_entries: Wire::dec(d)?,
            unverified_entries: Wire::dec(d)?,
            unknown_loss: Wire::dec(d)?,
        };
        if !value.valid() {
            return Err(invalid());
        }
        Ok(value)
    }
}
structure!(Source { path, archive_name });
structure!(Start { action, limits });
structure!(GuardedStart {
    start,
    ceiling,
    interactive
});
structure!(ResourceScope {
    output,
    entries,
    ratio
});
structure!(ResourceUsage {
    estimated_bytes,
    actual_bytes,
    estimated_entries,
    actual_entries,
    compressed_bytes,
    ratio_output_bytes,
    available_bytes
});
structure!(Entry {
    entry_id,
    name,
    kind,
    size,
    compressed_size,
    encrypted
});
structure!(Terminal {
    outcome,
    error,
    publication,
    created_count,
    retained_partial_count,
    durability,
    cleanup,
    detail
});
impl Wire for NativePath {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        match self {
            Self::Unix(v) => {
                1u8.enc(e)?;
                v.enc(e)
            }
            Self::Windows(v) => {
                2u8.enc(e)?;
                v.enc(e)
            }
        }
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        match u8::dec(d)? {
            1 => Ok(Self::Unix(Wire::dec(d)?)),
            2 => Ok(Self::Windows(Wire::dec(d)?)),
            _ => Err(invalid()),
        }
    }
}
impl Wire for SourceObject {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        match self {
            Self::Unix { device, inode } => {
                1u8.enc(e)?;
                device.enc(e)?;
                inode.enc(e)
            }
            Self::Windows {
                volume_serial,
                file_id,
            } => {
                2u8.enc(e)?;
                volume_serial.enc(e)?;
                file_id.enc(e)
            }
            Self::SplitSevenZip { manifest_sha256 } => {
                3u8.enc(e)?;
                manifest_sha256.enc(e)
            }
        }
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        match u8::dec(d)? {
            1 => Ok(Self::Unix {
                device: Wire::dec(d)?,
                inode: Wire::dec(d)?,
            }),
            2 => Ok(Self::Windows {
                volume_serial: Wire::dec(d)?,
                file_id: Wire::dec(d)?,
            }),
            3 => Ok(Self::SplitSevenZip {
                manifest_sha256: Wire::dec(d)?,
            }),
            _ => Err(invalid()),
        }
    }
}
pub(crate) fn encode_source_object(value: &SourceObject) -> io::Result<Vec<u8>> {
    encode_component(value)
}
pub(crate) fn encode_native_path(value: &NativePath) -> io::Result<Vec<u8>> {
    encode_component(value)
}
fn encode_component(value: &impl Wire) -> io::Result<Vec<u8>> {
    let mut encoder = Encoder {
        bytes: Vec::new(),
        selection_limit: LEGACY_VECTOR_ITEMS,
    };
    value.enc(&mut encoder)?;
    Ok(encoder.bytes)
}
impl Wire for Selection {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        match self {
            Self::All => 0u8.enc(e),
            Self::EntryIds(ids) => {
                1u8.enc(e)?;
                encode_vec(ids, e.selection_limit, e)
            }
        }
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        match u8::dec(d)? {
            0 => Ok(Self::All),
            1 => Ok(Self::EntryIds(decode_vec(d.selection_limit, d)?)),
            _ => Err(invalid()),
        }
    }
}
impl Wire for Action {
    fn enc(&self, e: &mut Encoder) -> io::Result<()> {
        match self {
            Self::RecoverZip {
                archive,
                output_file,
            } => {
                5u8.enc(e)?;
                archive.enc(e)?;
                output_file.enc(e)
            }
            Self::List { archive } => {
                0u8.enc(e)?;
                archive.enc(e)
            }
            Self::Extract {
                archive,
                destination,
                selection,
                list_id,
                gzip_output_name,
            } => {
                1u8.enc(e)?;
                archive.enc(e)?;
                destination.enc(e)?;
                selection.enc(e)?;
                list_id.enc(e)?;
                gzip_output_name.enc(e)
            }
            Self::CreateZip {
                sources,
                output_file,
                method,
                encryption,
            } => {
                2u8.enc(e)?;
                sources.enc(e)?;
                output_file.enc(e)?;
                method.enc(e)?;
                encryption.enc(e)
            }
            Self::Preview {
                archive,
                entry_id,
                list_id,
            } => {
                3u8.enc(e)?;
                archive.enc(e)?;
                entry_id.enc(e)?;
                list_id.enc(e)
            }
            Self::CreateSevenZip {
                sources,
                output_file,
                method,
                solid,
                encryption,
            } => {
                4u8.enc(e)?;
                sources.enc(e)?;
                output_file.enc(e)?;
                method.enc(e)?;
                solid.enc(e)?;
                encryption.enc(e)
            }
        }
    }
    fn dec(d: &mut Decoder<'_>) -> io::Result<Self> {
        match u8::dec(d)? {
            0 => Ok(Self::List {
                archive: Wire::dec(d)?,
            }),
            1 => Ok(Self::Extract {
                archive: Wire::dec(d)?,
                destination: Wire::dec(d)?,
                selection: Wire::dec(d)?,
                list_id: Wire::dec(d)?,
                gzip_output_name: Wire::dec(d)?,
            }),
            2 => Ok(Self::CreateZip {
                sources: Wire::dec(d)?,
                output_file: Wire::dec(d)?,
                method: Wire::dec(d)?,
                encryption: Wire::dec(d)?,
            }),
            3 => Ok(Self::Preview {
                archive: Wire::dec(d)?,
                entry_id: Wire::dec(d)?,
                list_id: Wire::dec(d)?,
            }),
            5 => Ok(Self::RecoverZip {
                archive: Wire::dec(d)?,
                output_file: Wire::dec(d)?,
            }),
            4 => Ok(Self::CreateSevenZip {
                sources: Wire::dec(d)?,
                output_file: Wire::dec(d)?,
                method: Wire::dec(d)?,
                solid: Wire::dec(d)?,
                encryption: Wire::dec(d)?,
            }),
            _ => Err(invalid()),
        }
    }
}
fn valid_preview_chunk(offset: u64, length: usize) -> bool {
    (1..=MAX_PREVIEW_CHUNK_BYTES).contains(&length)
        && offset
            .checked_add(length as u64)
            .is_some_and(|end| end <= MAX_PREVIEW_RGBA_BYTES)
}
impl Message {
    fn encode(&self) -> io::Result<(u16, Vec<u8>)> {
        let mut e = Encoder {
            bytes: Vec::new(),
            selection_limit: LEGACY_VECTOR_ITEMS,
        };
        let kind = match self {
            Self::Start(v) => {
                if matches!(v.action, Action::Preview { .. }) {
                    return Err(invalid());
                }
                v.enc(&mut e)?;
                1
            }
            Self::Password {
                challenge_id,
                secret,
            } => {
                if secret.0.len() > 1024 || std::str::from_utf8(&secret.0).is_err() {
                    return Err(invalid());
                }
                challenge_id.enc(&mut e)?;
                secret.0.enc(&mut e)?;
                2
            }
            Self::Cancel => 3,
            Self::StartInteractive(v) => {
                if !matches!(&v.action, Action::Extract { .. }) {
                    return Err(invalid());
                }
                v.enc(&mut e)?;
                4
            }
            Self::ConflictReply {
                challenge_id,
                choice,
                apply_to_remaining,
            } => {
                challenge_id.enc(&mut e)?;
                choice.enc(&mut e)?;
                apply_to_remaining.enc(&mut e)?;
                5
            }
            Self::StartGuarded(v) => {
                if !v.valid() {
                    return Err(invalid());
                }
                e.selection_limit = LEGACY_VECTOR_ITEMS.max(v.ceiling.entries as usize);
                v.enc(&mut e)?;
                6
            }
            Self::ResourceReply {
                challenge_id,
                scope,
                allow,
            } => {
                challenge_id.enc(&mut e)?;
                scope.enc(&mut e)?;
                allow.enc(&mut e)?;
                7
            }
            Self::ResourceRequired {
                challenge_id,
                action,
                scope,
                usage,
                soft_limits,
                ceiling,
            } => {
                challenge_id.enc(&mut e)?;
                action.enc(&mut e)?;
                scope.enc(&mut e)?;
                usage.enc(&mut e)?;
                soft_limits.enc(&mut e)?;
                ceiling.enc(&mut e)?;
                110
            }
            Self::Accepted { action, encryption } => {
                if *action == ActionKind::RecoverZip && *encryption {
                    return Err(invalid());
                }
                action.enc(&mut e)?;
                encryption.enc(&mut e)?;
                100
            }
            Self::Entry(v) => {
                v.enc(&mut e)?;
                101
            }
            Self::ListEnd {
                entry_count,
                list_id,
            } => {
                entry_count.enc(&mut e)?;
                list_id.enc(&mut e)?;
                102
            }
            Self::PasswordRequired {
                challenge_id,
                attempt,
                purpose,
            } => {
                challenge_id.enc(&mut e)?;
                attempt.enc(&mut e)?;
                purpose.enc(&mut e)?;
                103
            }
            Self::Progress {
                stage,
                processed_bytes,
                total_bytes,
                completed_entries,
            } => {
                stage.enc(&mut e)?;
                processed_bytes.enc(&mut e)?;
                total_bytes.enc(&mut e)?;
                completed_entries.enc(&mut e)?;
                104
            }
            Self::Output {
                entry_id,
                relative_name,
                disposition,
            } => {
                entry_id.enc(&mut e)?;
                relative_name.enc(&mut e)?;
                disposition.enc(&mut e)?;
                105
            }
            Self::PreviewBegin(info) => {
                info.enc(&mut e)?;
                111
            }
            Self::PreviewChunk {
                entry_id,
                offset,
                data,
            } => {
                if !valid_preview_chunk(*offset, data.len()) {
                    return Err(invalid());
                }
                entry_id.enc(&mut e)?;
                offset.enc(&mut e)?;
                (data.len() as u32).enc(&mut e)?;
                e.put(data)?;
                112
            }
            Self::PreviewEnd {
                entry_id,
                total_bytes,
                sha256,
            } => {
                if *total_bytes > MAX_PREVIEW_RGBA_BYTES {
                    return Err(invalid());
                }
                entry_id.enc(&mut e)?;
                total_bytes.enc(&mut e)?;
                sha256.enc(&mut e)?;
                113
            }
            Self::VolumeProblem { reason, volume } => {
                if !volume.is_basename() {
                    return Err(invalid());
                }
                reason.enc(&mut e)?;
                volume.enc(&mut e)?;
                114
            }
            Self::RecoveryEntry(v) => {
                v.enc(&mut e)?;
                115
            }
            Self::RecoveryEnd(v) => {
                v.enc(&mut e)?;
                116
            }
            Self::Terminal(v) => {
                if v.detail.len() > 4096 {
                    return Err(invalid());
                }
                v.enc(&mut e)?;
                106
            }
            Self::CancelAck {
                request_sequence,
                status,
            } => {
                request_sequence.enc(&mut e)?;
                status.enc(&mut e)?;
                107
            }
            Self::ConflictRequired {
                challenge_id,
                entry_id,
                relative_name,
                incoming_size,
                existing_size,
            } => {
                challenge_id.enc(&mut e)?;
                entry_id.enc(&mut e)?;
                relative_name.enc(&mut e)?;
                incoming_size.enc(&mut e)?;
                existing_size.enc(&mut e)?;
                108
            }
            Self::ConflictResolved {
                entry_id,
                relative_name,
                choice,
            } => {
                entry_id.enc(&mut e)?;
                relative_name.enc(&mut e)?;
                choice.enc(&mut e)?;
                109
            }
        };
        Ok((kind, e.bytes))
    }
    fn decode(kind: u16, b: &[u8]) -> io::Result<Self> {
        let mut d = Decoder {
            bytes: b,
            at: 0,
            selection_limit: if kind == 6 {
                MAX_SELECTION_IDS
            } else {
                LEGACY_VECTOR_ITEMS
            },
        };
        let m = match kind {
            1 => {
                let start = Start::dec(&mut d)?;
                if matches!(start.action, Action::Preview { .. }) {
                    return Err(invalid());
                }
                Self::Start(start)
            }
            2 => {
                let challenge_id = Wire::dec(&mut d)?;
                let secret = Secret(Vec::<u8>::dec(&mut d)?);
                if secret.0.len() > 1024 || std::str::from_utf8(&secret.0).is_err() {
                    return Err(invalid());
                }
                Self::Password {
                    challenge_id,
                    secret,
                }
            }
            3 => Self::Cancel,
            4 => {
                let start = Start::dec(&mut d)?;
                if !matches!(&start.action, Action::Extract { .. }) {
                    return Err(invalid());
                }
                Self::StartInteractive(start)
            }
            5 => Self::ConflictReply {
                challenge_id: Wire::dec(&mut d)?,
                choice: Wire::dec(&mut d)?,
                apply_to_remaining: Wire::dec(&mut d)?,
            },
            6 => {
                let start = GuardedStart::dec(&mut d)?;
                let limit = LEGACY_VECTOR_ITEMS.max(start.ceiling.entries as usize);
                if !start.valid()
                    || matches!(
                        &start.start.action,
                        Action::Extract { selection: Selection::EntryIds(ids), .. } if ids.len() > limit
                    )
                {
                    return Err(invalid());
                }
                Self::StartGuarded(start)
            }
            7 => Self::ResourceReply {
                challenge_id: Wire::dec(&mut d)?,
                scope: Wire::dec(&mut d)?,
                allow: Wire::dec(&mut d)?,
            },
            110 => Self::ResourceRequired {
                challenge_id: Wire::dec(&mut d)?,
                action: Wire::dec(&mut d)?,
                scope: Wire::dec(&mut d)?,
                usage: Wire::dec(&mut d)?,
                soft_limits: Wire::dec(&mut d)?,
                ceiling: Wire::dec(&mut d)?,
            },
            100 => {
                let action = Wire::dec(&mut d)?;
                let encryption = Wire::dec(&mut d)?;
                if action == ActionKind::RecoverZip && encryption {
                    return Err(invalid());
                }
                Self::Accepted { action, encryption }
            }
            115 => Self::RecoveryEntry(Wire::dec(&mut d)?),
            116 => Self::RecoveryEnd(Wire::dec(&mut d)?),
            101 => Self::Entry(Wire::dec(&mut d)?),
            102 => Self::ListEnd {
                entry_count: Wire::dec(&mut d)?,
                list_id: Wire::dec(&mut d)?,
            },
            103 => Self::PasswordRequired {
                challenge_id: Wire::dec(&mut d)?,
                attempt: Wire::dec(&mut d)?,
                purpose: Wire::dec(&mut d)?,
            },
            104 => Self::Progress {
                stage: Wire::dec(&mut d)?,
                processed_bytes: Wire::dec(&mut d)?,
                total_bytes: Wire::dec(&mut d)?,
                completed_entries: Wire::dec(&mut d)?,
            },
            105 => Self::Output {
                entry_id: Wire::dec(&mut d)?,
                relative_name: Wire::dec(&mut d)?,
                disposition: Wire::dec(&mut d)?,
            },
            111 => Self::PreviewBegin(Wire::dec(&mut d)?),
            112 => {
                let entry_id = Wire::dec(&mut d)?;
                let offset = Wire::dec(&mut d)?;
                let length = u32::dec(&mut d)? as usize;
                if !valid_preview_chunk(offset, length) {
                    return Err(invalid());
                }
                let bytes = d.take(length)?;
                let mut data = Vec::new();
                data.try_reserve_exact(length).map_err(|_| invalid())?;
                data.extend_from_slice(bytes);
                Self::PreviewChunk {
                    entry_id,
                    offset,
                    data,
                }
            }
            113 => {
                let entry_id = Wire::dec(&mut d)?;
                let total_bytes = u64::dec(&mut d)?;
                if total_bytes > MAX_PREVIEW_RGBA_BYTES {
                    return Err(invalid());
                }
                Self::PreviewEnd {
                    entry_id,
                    total_bytes,
                    sha256: Wire::dec(&mut d)?,
                }
            }
            114 => {
                let reason = Wire::dec(&mut d)?;
                let volume = NativePath::dec(&mut d)?;
                if !volume.is_basename() {
                    return Err(invalid());
                }
                Self::VolumeProblem { reason, volume }
            }
            106 => {
                let t = Terminal::dec(&mut d)?;
                if t.detail.len() > 4096 {
                    return Err(invalid());
                }
                Self::Terminal(t)
            }
            107 => Self::CancelAck {
                request_sequence: Wire::dec(&mut d)?,
                status: Wire::dec(&mut d)?,
            },
            108 => Self::ConflictRequired {
                challenge_id: Wire::dec(&mut d)?,
                entry_id: Wire::dec(&mut d)?,
                relative_name: Wire::dec(&mut d)?,
                incoming_size: Wire::dec(&mut d)?,
                existing_size: Wire::dec(&mut d)?,
            },
            109 => Self::ConflictResolved {
                entry_id: Wire::dec(&mut d)?,
                relative_name: Wire::dec(&mut d)?,
                choice: Wire::dec(&mut d)?,
            },
            _ => return Err(invalid()),
        };
        if d.at != b.len() {
            return Err(invalid());
        }
        Ok(m)
    }
}
pub struct Reader<R> {
    input: R,
    sequence: u64,
    job: Option<u64>,
    terminal: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(input: R) -> Self {
        Self {
            input,
            sequence: 0,
            job: None,
            terminal: false,
        }
    }
    pub fn read(&mut self) -> io::Result<Option<(u64, u64, Message)>> {
        let mut h = [0u8; 32];
        loop {
            match self.input.read(&mut h[..1]) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        if self.terminal {
            return Err(invalid());
        }
        self.input.read_exact(&mut h[1..])?;
        let len = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
        let job = u64::from_le_bytes(h[16..24].try_into().unwrap());
        let seq = u64::from_le_bytes(h[24..32].try_into().unwrap());
        if &h[..4] != b"ZAW1"
            || h[4..6] != 1u16.to_le_bytes()
            || h[12..16] != [0; 4]
            || len > MAX_BODY
            || job == 0
            || self.job.is_some_and(|j| j != job)
            || seq != self.sequence
            || seq >= MAX_FRAMES
        {
            return Err(invalid());
        }
        self.job = Some(job);
        self.sequence += 1;
        let mut b = zeroize::Zeroizing::new(vec![0; len]);
        self.input.read_exact(&mut b)?;
        let message = Message::decode(u16::from_le_bytes(h[6..8].try_into().unwrap()), &b)?;
        self.terminal = matches!(message, Message::Terminal(_));
        Ok(Some((job, seq, message)))
    }
}
pub struct Writer<W> {
    output: W,
    sequence: u64,
    job: u64,
    terminal: bool,
}
impl<W: Write> Writer<W> {
    pub fn new(output: W, job: u64) -> Self {
        Self {
            output,
            sequence: 0,
            job,
            terminal: false,
        }
    }
    pub fn send(&mut self, m: &Message) -> io::Result<()> {
        if self.job == 0 || self.terminal || self.sequence >= MAX_FRAMES {
            return Err(invalid());
        }
        let (kind, b) = m.encode()?;
        let b = zeroize::Zeroizing::new(b);
        let mut h = Vec::with_capacity(32);
        h.extend(b"ZAW1");
        h.extend(1u16.to_le_bytes());
        h.extend(kind.to_le_bytes());
        h.extend((b.len() as u32).to_le_bytes());
        h.extend(0u32.to_le_bytes());
        h.extend(self.job.to_le_bytes());
        h.extend(self.sequence.to_le_bytes());
        self.output.write_all(&h)?;
        self.output.write_all(&b)?;
        self.output.flush()?;
        self.sequence += 1;
        self.terminal = matches!(m, Message::Terminal(_));
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/selection_wire.rs"]
mod tests;
