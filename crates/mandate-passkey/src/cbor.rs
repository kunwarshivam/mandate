//! The CBOR subset WebAuthn responses use (RFC 8949): integers, byte and text strings, arrays,
//! maps, booleans, and null, with definite lengths only. Tags, floats, indefinite lengths,
//! duplicate map keys, and nesting deeper than [`MAX_DEPTH`] are refused (DEC-660 item 4).

/// Deepest nesting read below the outermost item; a COSE key or an extensions map needs two.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cbor {
    Uint(u64),
    /// Major type 1: the value is `-1 - n`.
    Nint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Cbor>),
    Map(Vec<(Cbor, Cbor)>),
    Bool(bool),
    Null,
}

impl Cbor {
    /// The integer value, when it is one that fits an `i64`.
    pub(crate) fn as_int(&self) -> Option<i64> {
        match self {
            Self::Uint(n) => i64::try_from(*n).ok(),
            Self::Nint(n) => i64::try_from(*n).ok().and_then(|n| (-1_i64).checked_sub(n)),
            _ => None,
        }
    }

    /// The value under `key` in a map.
    pub(crate) fn get(&self, key: &Cbor) -> Option<&Cbor> {
        match self {
            Self::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Malformed;

/// The item at the front of `input` and the bytes after it.
pub(crate) fn read(input: &[u8]) -> Result<(Cbor, &[u8]), Malformed> {
    item(input, 0)
}

/// The one item `input` holds, with nothing after it.
pub(crate) fn read_all(input: &[u8]) -> Result<Cbor, Malformed> {
    match read(input)? {
        (value, []) => Ok(value),
        _ => Err(Malformed),
    }
}

fn item(input: &[u8], depth: usize) -> Result<(Cbor, &[u8]), Malformed> {
    if depth > MAX_DEPTH {
        return Err(Malformed);
    }
    let (&initial, rest) = input.split_first().ok_or(Malformed)?;
    match initial {
        0xf4 => return Ok((Cbor::Bool(false), rest)),
        0xf5 => return Ok((Cbor::Bool(true), rest)),
        0xf6 => return Ok((Cbor::Null, rest)),
        _ => {}
    }
    let (n, rest) = argument(initial & 0x1f, rest)?;
    let inner = depth.checked_add(1).ok_or(Malformed)?;
    match initial >> 5 {
        0 => Ok((Cbor::Uint(n), rest)),
        1 => Ok((Cbor::Nint(n), rest)),
        2 => take(rest, n).map(|(bytes, rest)| (Cbor::Bytes(bytes.to_vec()), rest)),
        3 => {
            let (bytes, rest) = take(rest, n)?;
            let text = std::str::from_utf8(bytes).map_err(|_| Malformed)?;
            Ok((Cbor::Text(text.to_owned()), rest))
        }
        4 => {
            let mut items = Vec::new();
            let mut rest = rest;
            for _ in 0..n {
                let (value, after) = item(rest, inner)?;
                items.push(value);
                rest = after;
            }
            Ok((Cbor::Array(items), rest))
        }
        5 => {
            let mut entries: Vec<(Cbor, Cbor)> = Vec::new();
            let mut rest = rest;
            for _ in 0..n {
                let (key, after) = item(rest, inner)?;
                let (value, after) = item(after, inner)?;
                if entries.iter().any(|(k, _)| *k == key) {
                    return Err(Malformed);
                }
                entries.push((key, value));
                rest = after;
            }
            Ok((Cbor::Map(entries), rest))
        }
        _ => Err(Malformed),
    }
}

/// The argument of an initial byte whose low five bits are `info`, read big-endian. A longer
/// form than the value needs is read like the shortest one.
fn argument(info: u8, rest: &[u8]) -> Result<(u64, &[u8]), Malformed> {
    match info {
        0..=23 => Ok((u64::from(info), rest)),
        24 => {
            let (&n, rest) = rest.split_first().ok_or(Malformed)?;
            Ok((u64::from(n), rest))
        }
        25 => {
            let (n, rest) = rest.split_first_chunk::<2>().ok_or(Malformed)?;
            Ok((u64::from(u16::from_be_bytes(*n)), rest))
        }
        26 => {
            let (n, rest) = rest.split_first_chunk::<4>().ok_or(Malformed)?;
            Ok((u64::from(u32::from_be_bytes(*n)), rest))
        }
        27 => {
            let (n, rest) = rest.split_first_chunk::<8>().ok_or(Malformed)?;
            Ok((u64::from_be_bytes(*n), rest))
        }
        _ => Err(Malformed),
    }
}

fn take(input: &[u8], n: u64) -> Result<(&[u8], &[u8]), Malformed> {
    let n = usize::try_from(n).map_err(|_| Malformed)?;
    input.split_at_checked(n).ok_or(Malformed)
}
