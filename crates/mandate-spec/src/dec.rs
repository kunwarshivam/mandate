//! The document's decimals: text, checked against the grammar the schema declares for the field it
//! came from ([mandate spec §3](../../../docs/specs/mandate.md#3-structure), DEC-128 item 3).

use core::cmp::Ordering;

use mandate_canon::DecStr;
use mandate_num::{NumError, Ratio, Usd};

/// The schema's bound on either side of the point: `[0-9]{0,27}` after a first digit.
const MAX_PART_DIGITS: usize = 28;

/// Which `$defs` grammar of [`schemas/mandate.schema.json`](../../../schemas/mandate.schema.json) a
/// field declares. Each is a distinct set of strings, so the grammar is part of what a field's value
/// *is* rather than a bound checked afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DecGrammar {
    /// `decimal`: signed, and the **only** grammar that is. Its `$def` carries
    /// `"not": {"const": "-0"}` beside the pattern, so `-0` is not in the grammar although the
    /// pattern matches it. Referenced once, by `behavior.signal_models[].params[].value`.
    Decimal,
    /// `positive_decimal`: above zero, unsigned.
    PositiveDecimal,
    /// `fraction`: `0`, `1`, or a value strictly between them — the closed unit interval.
    Fraction,
    /// `open_fraction`: strictly between zero and one.
    OpenFraction,
    /// `unit_positive`: above zero and at most one.
    UnitPositive,
}

impl DecGrammar {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::PositiveDecimal => "positive_decimal",
            Self::Fraction => "fraction",
            Self::OpenFraction => "open_fraction",
            Self::UnitPositive => "unit_positive",
        }
    }

    /// True for the one grammar that admits a negative value.
    pub fn is_signed(self) -> bool {
        matches!(self, Self::Decimal)
    }
}

/// A decimal exactly as the document holds it, together with the grammar it satisfied.
///
/// **Not** a [`DecStr`], which is the journal grammar (journal spec §4.6) and *normalises* rather
/// than rejecting: `DecStr::parse("0.020")` is `Ok("0.02")`, it also takes `007.50`, `1e3`, and
/// `.5`, and it allows 29 integer digits where the schema allows 28. A document parsed through
/// `DecStr` alone would accept values the schema rejects (MC-S04, MC-S17), and the round trip the
/// version hash rests on would not hold.
///
/// [`SchemaDec::parse`] checks the whole `$def` — the pattern **and**, for [`DecGrammar::Decimal`],
/// the `-0` exclusion beside it — before it wraps anything. Every schema grammar is canonical in
/// consequence: no leading zero beyond a bare `0`, a fractional part ending in a non-zero digit, no
/// exponent, no `-0`. The text is therefore already `DecStr`'s normal form and
/// [`SchemaDec::to_dec_str`] is the identity, which is what lets a mandate's canonical bytes, and so
/// its version hash, reproduce the document the owner confirmed (ES-22).
///
/// Scale is why the type holds text at all: the grammars reach 28 integer and 28 fractional digits,
/// which [`Usd`] (28 places of scale over a 96-bit significand), [`Ratio`] (24 places), and `Price`
/// (9) cannot all hold. A rule converts what it computes with, and a value its target type cannot
/// hold exactly is an error naming the path, never a rounded number (DEC-128 item 4).
#[derive(Debug, Clone)]
pub struct SchemaDec {
    text: String,
    grammar: DecGrammar,
}

/// Equality is **by value**, so it agrees with [`Ord`].
///
/// The grammar is a witness of which field's `$def` admitted the text, not part of what the number
/// *is*: two fields may hold the same value, and `0.5` read as a `fraction` and `0.5` read as an
/// `open_fraction` are the same decimal. Deriving equality over `(text, grammar)` while ordering by
/// value alone would break [`Ord`]'s contract — `cmp` would say `Equal` where `==` said false, so a
/// `BTreeSet` would hold both, `dedup` would keep both, and `max` could pick either. A policy value
/// compared against a mandate field is exactly that cross-grammar case.
///
/// There is deliberately **no `Hash`**. ES-21 forbids `HashMap` and `HashSet` in this workspace
/// because their iteration order breaks replay, so a hash of a document value has no caller that is
/// allowed to exist — and an impl kept only to satisfy a derive is one more thing that has to agree
/// with `Eq` and that nothing would notice going wrong.
impl PartialEq for SchemaDec {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for SchemaDec {}

/// A decimal split into the parts the grammars constrain.
struct Parts<'a> {
    negative: bool,
    integer: &'a [u8],
    fraction: Option<&'a [u8]>,
}

/// Text that is not in the grammar its field declares.
///
/// Path-less on purpose: [`SchemaDec`] does not know which field it is being parsed for. The document
/// parser adds the pointer, producing [`ParseError::OffGrammar`], so a rejection still names the field
/// an author has to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not in the `{grammar}` grammar", grammar = self.grammar.as_str())]
pub struct GrammarMismatch {
    pub grammar: DecGrammar,
}

impl SchemaDec {
    /// The text if it is in `grammar`, else [`GrammarMismatch`].
    pub fn parse(text: &str, grammar: DecGrammar) -> Result<Self, GrammarMismatch> {
        let parts = split(text.as_bytes()).ok_or(GrammarMismatch { grammar })?;
        if !allowed(&parts, grammar) {
            return Err(GrammarMismatch { grammar });
        }
        Ok(Self {
            text: text.to_owned(),
            grammar,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn grammar(&self) -> DecGrammar {
        self.grammar
    }

    /// True when the value is zero. Only `0` is, because no grammar admits a trailing zero and
    /// `-0` is excluded.
    pub fn is_zero(&self) -> bool {
        self.text == "0"
    }

    pub fn is_negative(&self) -> bool {
        self.text.starts_with('-')
    }

    /// The same digits as a journal decimal, and the same text: the identity, because the grammar is
    /// canonical. Asserted as a property rather than assumed.
    pub fn to_dec_str(&self) -> Result<DecStr, GrammarMismatch> {
        DecStr::parse(&self.text).map_err(|_| GrammarMismatch {
            grammar: self.grammar,
        })
    }

    /// The value as dollars, exactly, or the numeric error the rule that asked turns into
    /// [`SpecError::OutOfRange`](crate::SpecError::OutOfRange) with the field's pointer.
    pub fn to_usd(&self) -> Result<Usd, NumError> {
        Usd::parse(&self.text)
    }

    /// The value as a ratio, exactly, with the same rule for a value [`Ratio`] cannot hold.
    pub fn to_ratio(&self) -> Result<Ratio, NumError> {
        Ratio::parse(&self.text)
    }
}

/// Splits a decimal into sign, integer digits, and fractional digits, rejecting anything the schema
/// patterns reject in common: an empty part, a leading zero beyond a bare `0`, a fractional part
/// ending in `0`, an exponent, a stray character, or a part past 28 digits.
fn split(bytes: &[u8]) -> Option<Parts<'_>> {
    let (negative, rest) = match bytes.split_first() {
        Some((b'-', tail)) => (true, tail),
        _ => (false, bytes),
    };
    let point = rest.iter().position(|b| *b == b'.');
    let (integer, fraction) = match point {
        Some(at) => (
            rest.get(..at)?,
            Some(rest.get(at.checked_add(1)?..).filter(|f| !f.is_empty())?),
        ),
        None => (rest, None),
    };
    if !digits_only(integer) || integer.is_empty() || integer.len() > MAX_PART_DIGITS {
        return None;
    }
    if integer.first() == Some(&b'0') && integer.len() > 1 {
        return None;
    }
    if fraction
        .is_some_and(|f| !digits_only(f) || f.len() > MAX_PART_DIGITS || f.last() == Some(&b'0'))
    {
        return None;
    }
    Some(Parts {
        negative,
        integer,
        fraction,
    })
}

fn digits_only(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

/// The per-grammar part of each `$def`, on top of the common shape [`split`] enforces.
///
/// [`DecGrammar::Decimal`] is the signed one, and its arm is the `not` clause beside the pattern:
/// `-0` matches the pattern and is excluded by the `$def`, so it is excluded here.
fn allowed(parts: &Parts<'_>, grammar: DecGrammar) -> bool {
    let zero_integer = parts.integer == b"0";
    let one = parts.integer == b"1" && parts.fraction.is_none();
    let zero = zero_integer && parts.fraction.is_none();
    match grammar {
        DecGrammar::Decimal => !(parts.negative && zero),
        DecGrammar::PositiveDecimal => !parts.negative && !zero,
        DecGrammar::Fraction => !parts.negative && (zero || one || zero_integer),
        DecGrammar::OpenFraction => !parts.negative && zero_integer && parts.fraction.is_some(),
        DecGrammar::UnitPositive => {
            !parts.negative && (one || (zero_integer && parts.fraction.is_some()))
        }
    }
}

impl core::fmt::Display for SchemaDec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.text)
    }
}

/// Decimal order, total and exact.
///
/// Signs first; then, between two values of the same sign, the integer part by length and then by
/// text, and the fractional digits by text — **reversed between two negatives**, where the larger
/// magnitude is the smaller number. Length then text is enough because the grammar forbids a leading
/// zero, and plain text comparison is enough on the fractional digits because it forbids a trailing
/// one. Only [`DecGrammar::Decimal`] admits a negative, so only a signal-model parameter reaches that
/// branch; V-012, V-013, and V-014 compare unsigned fields (DEC-128 item 3).
impl Ord for SchemaDec {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_negative(), other.is_negative()) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => magnitude(&self.text, &other.text),
            (true, true) => magnitude(&self.text, &other.text).reverse(),
        }
    }
}

impl PartialOrd for SchemaDec {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Compares two canonical decimals by absolute value, ignoring any sign.
fn magnitude(a: &str, b: &str) -> Ordering {
    let strip = |t: &str| -> (String, String) {
        let body = t.strip_prefix('-').unwrap_or(t);
        match body.split_once('.') {
            Some((i, f)) => (i.to_owned(), f.to_owned()),
            None => (body.to_owned(), String::new()),
        }
    };
    let (ai, af) = strip(a);
    let (bi, bf) = strip(b);
    ai.len()
        .cmp(&bi.len())
        .then_with(|| ai.cmp(&bi))
        .then_with(|| af.cmp(&bf))
}
