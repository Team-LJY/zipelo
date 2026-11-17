use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{ffi::OsStrExt, fs::FileTypeExt, io::FromRawFd, net::UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

// 중복 키는 해석 순서에 따라 판정이 달라질 수 있어 모든 깊이에서 거부함.
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Strict(Value::Number(v)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(Value::String(v.into())))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(v)) = seq.next_element()? {
                    values.push(v);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
                let mut values = Map::new();
                while let Some((key, Strict(v))) = map.next_entry::<String, Strict>()? {
                    if values.insert(key, v).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(Strict(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Json)
    }
}

type Check<T> = Result<T, ()>;
const MAX_FRAME: usize = 1024 * 1024;
const MAX_TOTAL: usize = 16 * MAX_FRAME;
const MAX_RECORDS: usize = 65_536;

fn directory(path: &Path) -> Check<bool> {
    let mut prefix = std::path::PathBuf::new();
    for part in path.components() {
        prefix.push(part.as_os_str());
        match fs::symlink_metadata(&prefix) {
            Ok(m) if m.is_dir() => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            _ => return Err(()),
        }
    }
    Ok(true)
}

fn name(value: &Value) -> Check<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or(())
}
fn id(value: &Value) -> Check<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v != u32::MAX)
        .ok_or(())
}
fn strings(value: Option<&Value>) -> Check<Vec<&str>> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => values.iter().map(name).collect(),
        _ => Err(()),
    }
}
fn record(parameters: &Value) -> Check<&Value> {
    match parameters.get("incomplete") {
        None | Some(Value::Bool(false)) => {}
        _ => return Err(()),
    }
    let record = parameters
        .get("record")
        .filter(|v| v.is_object())
        .ok_or(())?;
    // 기기별 덮어쓰기를 해석하지 않은 상태에서는 거부함.
    for key in ["binding", "perMachine"] {
        match record.get(key) {
            None => {}
            Some(Value::Object(v)) if key == "binding" && v.is_empty() => {}
            Some(Value::Array(v)) if key == "perMachine" && v.is_empty() => {}
            _ => return Err(()),
        }
    }
    Ok(record)
}

struct Budget {
    deadline: Instant,
    bytes: usize,
    records: usize,
}
impl Budget {
    fn remaining(&self) -> Check<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(())
    }
}

// Unix 소켓의 대기열이 가득 차도 연결 단계에서 무한히 기다리지 않음.
fn connect(path: &Path) -> Check<UnixStream> {
    #[repr(C)]
    struct Address {
        family: u16,
        path: [u8; 108],
    }
    unsafe extern "C" {
        fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        fn connect(fd: i32, address: *const Address, size: u32) -> i32;
    }
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) || bytes.len() >= 108 {
        return Err(());
    }
    let mut address = Address {
        family: 1,
        path: [0; 108],
    };
    address.path[..bytes.len()].copy_from_slice(bytes);
    let fd = unsafe { socket(1, 1 | 0x80000 | 0x800, 0) };
    if fd < 0 {
        return Err(());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    if unsafe { connect(fd, &address, (2 + bytes.len() + 1) as u32) } != 0 {
        return Err(());
    }
    stream.set_nonblocking(false).map_err(|_| ())?;
    Ok(stream)
}

fn query(
    path: &Path,
    service: &str,
    method: &str,
    budget: &mut Budget,
    mut check: impl FnMut(&Value) -> Check<()>,
) -> Check<()> {
    let mut stream = connect(path)?;
    stream
        .set_write_timeout(Some(budget.remaining()?))
        .map_err(|_| ())?;
    let request = serde_json::json!({
        "method": format!("io.systemd.UserDatabase.{method}"),
        "parameters": {"service": service}, "more": true,
    });
    stream
        .write_all(request.to_string().as_bytes())
        .map_err(|_| ())?;
    stream.write_all(&[0]).map_err(|_| ())?;
    let mut reader = BufReader::new(stream);
    loop {
        let mut frame = Vec::new();
        loop {
            reader
                .get_ref()
                .set_read_timeout(Some(budget.remaining()?))
                .map_err(|_| ())?;
            let chunk = reader.fill_buf().map_err(|_| ())?;
            if chunk.is_empty() {
                return Err(());
            }
            let end = chunk.iter().position(|b| *b == 0);
            let n = end.map_or(chunk.len(), |i| i + 1);
            budget.bytes = budget
                .bytes
                .checked_add(n)
                .filter(|n| *n <= MAX_TOTAL)
                .ok_or(())?;
            if frame.len() + n > MAX_FRAME {
                return Err(());
            }
            frame.extend_from_slice(&chunk[..n]);
            reader.consume(n);
            if end.is_some() {
                frame.pop();
                break;
            }
        }
        budget.records += 1;
        if budget.records > MAX_RECORDS {
            return Err(());
        }
        let Strict(reply) = serde_json::from_slice::<Strict>(&frame).map_err(|_| ())?;
        let object = reply.as_object().ok_or(())?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "parameters" | "error" | "continues"))
        {
            return Err(());
        }
        let continues = match reply.get("continues") {
            None | Some(Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            _ => return Err(()),
        };
        if let Some(error) = reply.get("error") {
            return if error.as_str() == Some("io.systemd.UserDatabase.NoRecordFound")
                && !continues
                && reply
                    .get("parameters")
                    .is_none_or(|v| v.as_object().is_some_and(|v| v.is_empty()))
            {
                Ok(())
            } else {
                Err(())
            };
        }
        let parameters = reply
            .get("parameters")
            .filter(|v| v.is_object())
            .ok_or(())?;
        check(parameters)?;
        if !continues {
            return Ok(());
        }
    }
}

fn inspect(gid: u32, uid: u32, owner: &str, sockets: &Path, dropins: &[&Path]) -> Check<()> {
    for path in dropins {
        if directory(path)? && fs::read_dir(path).map_err(|_| ())?.next().is_some() {
            return Err(());
        }
    }
    let mut groups = BTreeMap::<String, u32>::new();
    let local = fs::read_to_string("/etc/group").map_err(|_| ())?;
    for line in local
        .lines()
        .filter(|v| !v.is_empty() && !v.starts_with('#'))
    {
        let fields: Vec<_> = line.split(':').collect();
        if fields.len() != 4 || fields[0].is_empty() {
            return Err(());
        }
        let group_gid = fields[2].parse::<u32>().map_err(|_| ())?;
        if group_gid == gid && fields[3].split(',').any(|v| !v.is_empty() && v != owner) {
            return Err(());
        }
        if groups.insert(fields[0].into(), group_gid).is_some() {
            return Err(());
        }
    }
    let mut services = Vec::new();
    if directory(sockets)? {
        for entry in fs::read_dir(sockets).map_err(|_| ())? {
            let entry = entry.map_err(|_| ())?;
            let service = entry.file_name().into_string().map_err(|_| ())?;
            // 중계 서비스도 소켓 형식인지는 확인함.
            if !entry.file_type().map_err(|_| ())?.is_socket() {
                return Err(());
            }
            if matches!(
                service.as_str(),
                "io.systemd.Multiplexer" | "io.systemd.NameServiceSwitch"
            ) {
                continue;
            }
            services.push((service, entry.path()));
            if services.len() > 256 {
                return Err(());
            }
        }
    }
    let mut budget = Budget {
        deadline: Instant::now() + Duration::from_secs(5),
        bytes: 0,
        records: 0,
    };
    // 모든 서비스의 그룹 이름을 먼저 모아 서비스 간 구성원도 검사함.
    for (service, path) in &services {
        query(path, service, "GetGroupRecord", &mut budget, |p| {
            let r = record(p)?;
            let group_name = name(&r["groupName"])?;
            let group_gid = id(&r["gid"])?;
            let members = strings(r.get("members"))?;
            if group_gid == gid && members.iter().any(|v| *v != owner) {
                return Err(());
            }
            if groups
                .insert(group_name.into(), group_gid)
                .is_some_and(|v| v != group_gid)
            {
                return Err(());
            }
            Ok(())
        })?;
    }
    for (service, path) in &services {
        query(path, service, "GetUserRecord", &mut budget, |p| {
            let r = record(p)?;
            let user = name(&r["userName"])?;
            let user_uid = id(&r["uid"])?;
            let primary = id(&r["gid"])?;
            let is_owner = user_uid == uid && user == owner;
            if primary == gid && !is_owner {
                return Err(());
            }
            for group in strings(r.get("memberOf"))? {
                let target = groups.get(group).ok_or(())?;
                if *target == gid && !is_owner {
                    return Err(());
                }
            }
            Ok(())
        })?;
        query(path, service, "GetMemberships", &mut budget, |p| {
            let user = name(&p["userName"])?;
            let group = name(&p["groupName"])?;
            let target = groups.get(group).ok_or(())?;
            if *target == gid && user != owner {
                Err(())
            } else {
                Ok(())
            }
        })?;
    }
    Ok(())
}

pub(crate) fn group_is_private(
    gid: u32,
    uid: u32,
    owner: &str,
    sockets: &Path,
    dropins: &[&Path],
) -> bool {
    inspect(gid, uid, owner, sockets, dropins).is_ok()
}
