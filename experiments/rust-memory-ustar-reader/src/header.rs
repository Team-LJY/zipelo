use crate::{Entry, Failure, Kind};

pub(crate) fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

pub(crate) fn copy(value: &str) -> Result<String, Failure> {
    let mut result = String::new();
    result
        .try_reserve_exact(value.len())
        .map_err(|_| Failure::Allocation)?;
    result.push_str(value);
    Ok(result)
}

fn text(field: &[u8]) -> Result<&str, Failure> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if !zero(&field[end..]) {
        return Err(Failure::Integrity);
    }
    std::str::from_utf8(&field[..end]).map_err(|_| Failure::Integrity)
}

fn octal(field: &[u8], allow_empty: bool) -> Result<u64, Failure> {
    let mut value = 0_u64;
    let mut digits = 0;
    let mut ended = false;
    for &byte in field {
        if byte == 0 || byte == b' ' {
            if digits > 0 || byte == 0 {
                ended = true;
            }
            continue;
        }
        if ended || !(b'0'..=b'7').contains(&byte) {
            return Err(Failure::Integrity);
        }
        let digit = u64::from(byte - b'0');
        if value > (i64::MAX as u64 - digit) / 8 {
            return Err(Failure::Integrity);
        }
        value = value * 8 + digit;
        digits += 1;
    }
    if digits == 0 && !allow_empty {
        return Err(Failure::Integrity);
    }
    Ok(value)
}

pub(crate) fn parse(h: &[u8], index: usize) -> Result<Entry, Failure> {
    let checksum = octal(&h[148..156], false)?;
    let sum: u64 = h
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    if checksum != sum {
        return Err(Failure::Integrity);
    }
    if &h[257..263] != b"ustar\0" || &h[263..265] != b"00" {
        return Err(Failure::Unsupported);
    }
    let kind = match h[156] {
        0 | b'0' => Kind::File,
        b'5' => Kind::Directory,
        _ => return Err(Failure::Unsupported),
    };
    for range in [100..108, 108..116, 116..124, 136..148] {
        octal(&h[range], false)?;
    }
    let size = octal(&h[124..136], false)?;
    if octal(&h[329..337], true)? != 0
        || octal(&h[337..345], true)? != 0
        || !zero(&h[157..257])
        || !zero(&h[500..512])
    {
        return Err(Failure::Integrity);
    }
    text(&h[265..297])?;
    text(&h[297..329])?;
    let leaf = text(&h[..100])?;
    let prefix = text(&h[345..500])?;
    if leaf.is_empty() {
        return Err(Failure::Integrity);
    }
    let mut name = String::new();
    name.try_reserve_exact(prefix.len() + 1 + leaf.len())
        .map_err(|_| Failure::Allocation)?;
    if !prefix.is_empty() {
        name.push_str(prefix);
        name.push('/');
    }
    name.push_str(leaf);
    if kind == Kind::Directory {
        if name.ends_with('/') {
            name.pop();
        }
        if size != 0 {
            return Err(Failure::Integrity);
        }
    }
    if name.is_empty()
        || name.encode_utf16().count() > 4096
        || name.contains(['\0', '\\'])
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.len() > 255)
    {
        return Err(Failure::UnsafePath);
    }
    // 순번은 정규화 없이 정확한 십진 문자열로 비교함.
    let mut digits = [0_u8; 20];
    let mut cursor = digits.len();
    let mut number = index;
    loop {
        cursor -= 1;
        digits[cursor] = b'0' + (number % 10) as u8;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    let id = copy(std::str::from_utf8(&digits[cursor..]).map_err(|_| Failure::Integrity)?)?;
    Ok(Entry {
        id,
        name,
        kind,
        size,
    })
}

pub(crate) fn unique(entry: &Entry, previous: &[Entry]) -> Result<(), Failure> {
    for other in previous {
        if entry.name == other.name
            || (other.kind == Kind::File && descendant(&entry.name, &other.name))
            || (entry.kind == Kind::File && descendant(&other.name, &entry.name))
        {
            return Err(Failure::Conflict);
        }
    }
    Ok(())
}

fn descendant(path: &str, ancestor: &str) -> bool {
    path.strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
