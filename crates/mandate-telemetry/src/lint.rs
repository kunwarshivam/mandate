//! The label lint (design §8.1): over the catalog, every metric's name, help text, kind suffix,
//! and label keys; over a scrape, every family, sample, label key, and label value, each against
//! the catalog.

use crate::ReasonCode;
use crate::catalog::{self, Kind, Metric};
use crate::label::LabelKey;

/// Words no metric name or help text may hold (design §8.1, `AGENTS.md` "Do not"): order details,
/// positions, money, mandate content, people, and secrets.
#[rustfmt::skip]
pub const FORBIDDEN_WORDS: [&str; 34] = [
    "symbol", "instrument", "ticker", "side", "buy", "sell", "qty", "quantity", "price",
    "notional", "position", "positions", "pnl", "profit", "loss", "cash", "balance", "equity",
    "exposure", "value", "amount", "allocation", "limit", "thesis", "score", "user", "person",
    "name", "email", "phone", "secret", "token", "password", "credential",
];

/// What the lint found. A finding names the metric as the catalog or the scrape spells it and
/// the label key, never a label value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A name that is not `mandate_` and then `[a-z0-9_]`.
    Name { metric: String },
    /// A name or help text holding one of [`FORBIDDEN_WORDS`].
    Forbidden { metric: String, word: &'static str },
    /// A name whose suffix does not match its kind.
    Suffix { metric: String },
    /// Two catalog entries with one name.
    Duplicate { metric: String },
    /// A metric declaring one label key twice, or out of [`LabelKey`] order.
    Labels { metric: String },
    /// A reason code that is not an opaque ID's shape.
    Reason { code: &'static str },
    /// A scraped family or sample that is not in the catalog.
    Unknown { metric: String },
    /// A scraped help text or type that is not the catalog's.
    Header { metric: String },
    /// A scraped label key outside the metric's declared set.
    Undeclared { metric: String, key: String },
    /// A scraped label value its key does not admit.
    Value { metric: String, key: &'static str },
    /// A scraped line that does not parse, by its 1-based number.
    Malformed { line: usize },
}

/// Every finding over `metrics`: each metric by [`metric`], and every name used twice.
pub fn catalog(metrics: &[Metric]) -> Vec<Finding> {
    let mut findings: Vec<Finding> = metrics
        .iter()
        .flat_map(|m| metric(m.name(), m.help(), m.kind(), m.labels()))
        .collect();
    for (i, m) in metrics.iter().enumerate() {
        if metrics
            .iter()
            .take(i)
            .any(|earlier| earlier.name() == m.name())
        {
            findings.push(Finding::Duplicate {
                metric: m.name().to_owned(),
            });
        }
    }
    findings
}

/// Every finding over one metric's declaration.
pub fn metric(name: &str, help: &str, kind: Kind, labels: &[LabelKey]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let owned = || name.to_owned();
    let Some(rest) = name.strip_prefix("mandate_") else {
        return vec![Finding::Name { metric: owned() }];
    };
    if rest.is_empty()
        || !rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        findings.push(Finding::Name { metric: owned() });
    }
    let suffix_ok = match kind {
        Kind::Counter => name.ends_with("_total"),
        Kind::State => !name.ends_with("_total"),
    };
    if !suffix_ok {
        findings.push(Finding::Suffix { metric: owned() });
    }
    let words = rest.split('_').chain(
        help.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty()),
    );
    for word in words {
        let lower = word.to_ascii_lowercase();
        if let Some(found) = FORBIDDEN_WORDS.into_iter().find(|f| *f == lower) {
            findings.push(Finding::Forbidden {
                metric: owned(),
                word: found,
            });
        }
    }
    if !labels
        .windows(2)
        .all(|pair| matches!(pair, [a, b] if a < b))
    {
        findings.push(Finding::Labels { metric: owned() });
    }
    findings
}

/// Every reason code in `codes` that is not an opaque ID's shape.
pub fn reasons(codes: &[ReasonCode]) -> Vec<Finding> {
    codes
        .iter()
        .filter(|code| ReasonCode::new(code.as_str()).is_err())
        .map(|code| Finding::Reason {
            code: code.as_str(),
        })
        .collect()
}

/// Every finding over a Prometheus text exposition: each `# HELP` and `# TYPE` line must be a
/// catalog metric's own; each sample a catalog metric's, carrying only that metric's declared
/// keys, each value one its key admits, and a number.
pub fn exposition(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# HELP ") {
            header(rest, number, &mut findings, |m, help| m.help() == help);
        } else if let Some(rest) = line.strip_prefix("# TYPE ") {
            header(rest, number, &mut findings, |m, kind| {
                type_of(m.kind()) == kind
            });
        } else if line.starts_with('#') {
            findings.push(Finding::Malformed { line: number });
        } else {
            sample(line, number, &mut findings);
        }
    }
    findings
}

const fn type_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Counter => "counter",
        Kind::State => "gauge",
    }
}

fn header(
    rest: &str,
    number: usize,
    findings: &mut Vec<Finding>,
    matches: impl Fn(&Metric, &str) -> bool,
) {
    let Some((name, tail)) = rest.split_once(' ') else {
        findings.push(Finding::Malformed { line: number });
        return;
    };
    match catalog::find(name) {
        None => findings.push(Finding::Unknown {
            metric: name.to_owned(),
        }),
        Some(metric) if !matches(metric, tail) => findings.push(Finding::Header {
            metric: name.to_owned(),
        }),
        Some(_) => {}
    }
}

fn sample(line: &str, number: usize, findings: &mut Vec<Finding>) {
    let name_end = line.find(['{', ' ']).unwrap_or(line.len());
    let (name, rest) = line.split_at(name_end);
    let Some(metric) = catalog::find(name) else {
        findings.push(Finding::Unknown {
            metric: name.to_owned(),
        });
        return;
    };
    let (pairs, value) = match rest.strip_prefix('{') {
        Some(body) => match label_pairs(body) {
            Some(parsed) => parsed,
            None => {
                findings.push(Finding::Malformed { line: number });
                return;
            }
        },
        None => (Vec::new(), rest),
    };
    if !is_sample_value(value.trim()) {
        findings.push(Finding::Malformed { line: number });
    }
    for (key, label_value) in pairs {
        match LabelKey::parse(&key).filter(|k| metric.labels().contains(k)) {
            None => findings.push(Finding::Undeclared {
                metric: name.to_owned(),
                key,
            }),
            Some(k) if !k.admits(&label_value) => findings.push(Finding::Value {
                metric: name.to_owned(),
                key: k.key(),
            }),
            Some(_) => {}
        }
    }
}

/// Parses `k="v",...}` and returns the pairs, unescaped as the text format escapes them (`\\`,
/// `\"`, `\n`), and the text after the `}`. Any other escape, a key outside `[a-z_]`, or a pair
/// not followed by `,` or `}` does not parse.
fn label_pairs(body: &str) -> Option<(Vec<(String, String)>, &str)> {
    let mut pairs = Vec::new();
    let mut rest = body;
    loop {
        if let Some(after) = rest.strip_prefix('}') {
            return Some((pairs, after));
        }
        let (key, after_key) = rest.split_once("=\"")?;
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return None;
        }
        let mut value = String::new();
        let mut chars = after_key.char_indices();
        let end = loop {
            match chars.next()? {
                (_, '\\') => match chars.next()?.1 {
                    'n' => value.push('\n'),
                    c @ ('\\' | '"') => value.push(c),
                    _ => return None,
                },
                (i, '"') => break i,
                (_, c) => value.push(c),
            }
        };
        pairs.push((key.to_owned(), value));
        let after_value = after_key.get(end.saturating_add(1)..)?;
        rest = match after_value.strip_prefix(',') {
            Some(next) => next,
            None if after_value.starts_with('}') => after_value,
            None => return None,
        };
    }
}

fn is_sample_value(text: &str) -> bool {
    matches!(text, "+Inf" | "-Inf" | "NaN") || text.parse::<f64>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::{Finding, reasons};
    use crate::ReasonCode;

    #[test]
    fn a_fixed_reason_code_outside_the_shape_is_a_finding() {
        let bad = ReasonCode::fixed("Not_Opaque");
        let good = ReasonCode::fixed("completed");
        assert_eq!(
            reasons(&[good, bad]),
            vec![Finding::Reason { code: "Not_Opaque" }]
        );
    }
}
