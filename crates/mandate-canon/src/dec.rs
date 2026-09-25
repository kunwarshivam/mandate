//! The journal decimal grammar (spec §4.6), normalized at the text level so nothing is ever rounded.

use crate::DecError;

const MAX_FRACTION_DIGITS: i64 = 28;
const MAX_INTEGER_DIGITS: i64 = 29;
/// 7.9 × 10²⁸, the exclusive bound on the absolute value; it has 29 integer digits.
const LIMIT: &str = "79000000000000000000000000000";

/// Parses sign, digits, and exponent, then writes value = digits × 10^exponent with the digits
/// free of leading and trailing zeros.
pub(crate) fn normalize(input: &str) -> Result<String, DecError> {
    let mut rest = input.as_bytes();
    let negative = eat(&mut rest, b'-');
    let int = digits(&mut rest);
    let frac = if eat(&mut rest, b'.') {
        digits(&mut rest)
    } else {
        &[]
    };
    if int.is_empty() && frac.is_empty() {
        return Err(DecError::Syntax);
    }
    let mut exponent = 0i64;
    if eat(&mut rest, b'e') || eat(&mut rest, b'E') {
        let negative_exp = eat(&mut rest, b'-');
        if !negative_exp {
            eat(&mut rest, b'+');
        }
        let exp_digits = digits(&mut rest);
        if exp_digits.is_empty() {
            return Err(DecError::Syntax);
        }
        exponent = exp_digits
            .iter()
            .try_fold(0i64, |e, d| {
                e.checked_mul(10)?
                    .checked_add(i64::from(d.checked_sub(b'0')?))
            })
            .ok_or(DecError::OutOfRange)?;
        if negative_exp {
            exponent = exponent.checked_neg().ok_or(DecError::OutOfRange)?;
        }
    }
    if !rest.is_empty() {
        return Err(DecError::Syntax);
    }

    let all: Vec<u8> = int.iter().chain(frac).copied().collect();
    let Some(first) = all.iter().position(|d| *d != b'0') else {
        return Ok("0".to_owned());
    };
    let last = all.iter().rposition(|d| *d != b'0').unwrap_or(first);
    let significant = all.get(first..=last).ok_or(DecError::Syntax)?;
    let trailing_zeros = all.len().saturating_sub(last.saturating_add(1));
    let shift = i64::try_from(trailing_zeros)
        .ok()
        .zip(i64::try_from(frac.len()).ok())
        .and_then(|(tz, f)| tz.checked_sub(f))
        .ok_or(DecError::OutOfRange)?;
    let exponent = exponent.checked_add(shift).ok_or(DecError::OutOfRange)?;

    let len = i64::try_from(significant.len()).map_err(|_| DecError::OutOfRange)?;
    let int_len = len.checked_add(exponent).ok_or(DecError::OutOfRange)?;
    let frac_len = exponent.checked_neg().ok_or(DecError::OutOfRange)?.max(0);
    if int_len > MAX_INTEGER_DIGITS || frac_len > MAX_FRACTION_DIGITS {
        return Err(DecError::OutOfRange);
    }

    let text = |d: &[u8]| String::from_utf8_lossy(d).into_owned();
    let (int_part, frac_part) = if exponent >= 0 {
        let zeros = usize::try_from(exponent).map_err(|_| DecError::OutOfRange)?;
        (text(significant) + &"0".repeat(zeros), String::new())
    } else if int_len > 0 {
        let split = usize::try_from(int_len).map_err(|_| DecError::OutOfRange)?;
        let (i, f) = significant
            .split_at_checked(split)
            .ok_or(DecError::OutOfRange)?;
        (text(i), text(f))
    } else {
        let zeros = int_len
            .checked_neg()
            .and_then(|z| usize::try_from(z).ok())
            .ok_or(DecError::OutOfRange)?;
        ("0".to_owned(), "0".repeat(zeros) + &text(significant))
    };
    if int_part.len() == LIMIT.len() && int_part.as_str() >= LIMIT {
        return Err(DecError::OutOfRange);
    }

    let mut out = String::with_capacity(
        int_part
            .len()
            .saturating_add(frac_part.len())
            .saturating_add(2),
    );
    if negative {
        out.push('-');
    }
    out.push_str(&int_part);
    if !frac_part.is_empty() {
        out.push('.');
        out.push_str(&frac_part);
    }
    Ok(out)
}

fn eat(rest: &mut &[u8], b: u8) -> bool {
    match rest.split_first() {
        Some((first, tail)) if *first == b => {
            *rest = tail;
            true
        }
        _ => false,
    }
}

fn digits<'a>(rest: &mut &'a [u8]) -> &'a [u8] {
    let all: &'a [u8] = rest;
    let n = all.iter().take_while(|b| b.is_ascii_digit()).count();
    let (digits, tail) = all.split_at_checked(n).unwrap_or((&[], all));
    *rest = tail;
    digits
}
