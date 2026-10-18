//! Masking a secret in a GitHub Actions log (DEC-2018 item 7). The repository is public, so its
//! Actions logs are, and GitHub masks only `secrets.*` values on its own. Every xtask path that
//! holds a secret in a workflow calls [`add_mask`] as soon as it has the value, so no later line
//! can print it in any form GitHub would not then hide.

use std::collections::BTreeSet;
use std::env;
use std::io::{self, Write as _};

use crate::image;

/// `data` escaped as a workflow command's data. GitHub decodes `%25`, `%0D` and `%0A` there, so
/// `%` is escaped first, then CR and LF; unescaped, a form holding a literal `%0A` would register
/// a different mask and leave itself visible.
pub(crate) fn command_escaped(data: &str) -> String {
    data.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Every form of `value` a log line could carry, none empty or whitespace only: the value, its
/// standard and URL-safe base64 with and without padding, its percent-encoding (every byte but
/// `[A-Za-z0-9-._~]`), its JSON-escaped form, and each of its lines at least
/// [`MIN_LINE_FORM`] characters long once trimmed.
/// The shortest line of a multi-line value masked on its own. A shorter line would hide every
/// match of it anywhere in the log, and is too short to be secret on its own; the whole value
/// and its encodings are masked whatever their length.
const MIN_LINE_FORM: usize = 4;

pub(crate) fn forms(value: &str) -> BTreeSet<String> {
    let base64 = image::base64(value.as_bytes());
    let base64url = base64.replace('+', "-").replace('/', "_");
    let mut url = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    let json = serde_json::Value::String(value.to_owned()).to_string();
    let json = json
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(&json)
        .to_owned();
    let mut all = vec![
        value.to_owned(),
        base64.trim_end_matches('=').to_owned(),
        base64url.trim_end_matches('=').to_owned(),
        base64,
        base64url,
        url,
        json,
    ];
    all.extend(
        value
            .lines()
            .filter(|line| line.trim().chars().count() >= MIN_LINE_FORM)
            .map(str::to_owned),
    );
    all.into_iter()
        .filter(|form| !form.trim().is_empty())
        .collect()
}

/// The `::add-mask::` command for each of `value`'s [`forms`], its data escaped.
pub(crate) fn commands(value: &str) -> Vec<String> {
    forms(value)
        .iter()
        .map(|form| format!("::add-mask::{}", command_escaped(form)))
        .collect()
}

/// Masks `value` in every form when this runs in GitHub Actions, and does nothing elsewhere.
pub(crate) fn add_mask(value: &str) {
    if env::var("GITHUB_ACTIONS").is_ok_and(|actions| actions == "true") {
        let mut out = io::stdout().lock();
        for command in commands(value) {
            let _ = writeln!(out, "{command}");
        }
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Command data decoded as GitHub decodes it, in one pass.
    fn decoded(data: &str) -> String {
        let mut out = String::new();
        let mut rest = data;
        while let Some(c) = rest.chars().next() {
            if let Some(tail) = rest.strip_prefix("%25") {
                out.push('%');
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("%0D") {
                out.push('\r');
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("%0A") {
                out.push('\n');
                rest = tail;
            } else {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        out
    }

    /// `%` is escaped first, then CR and LF, so a literal `%0A` survives as itself.
    #[test]
    fn command_data_escapes_percent_first_then_cr_and_lf() {
        assert_eq!(command_escaped("%0A"), "%250A");
        assert_eq!(command_escaped("a\rb\nc%0Ad%"), "a%0Db%0Ac%250Ad%25");
    }

    /// Each printed command is one line, and decodes to exactly one of the value's forms.
    #[test]
    fn each_command_is_one_line_and_decodes_to_its_form() {
        let value = "a%b\r\nc";
        let printed = commands(value);
        assert!(printed.contains(&"::add-mask::a%25b%0D%0Ac".to_owned()));
        assert!(printed.contains(&"::add-mask::a%2525b%250D%250Ac".to_owned()));
        let value = "p%0Aq\rr%0Ds%\nline two";
        let forms = forms(value);
        let printed = commands(value);
        assert_eq!(printed.len(), forms.len());
        for line in &printed {
            assert!(!line.contains(['\n', '\r']), "{line:?} spans lines");
            let data = line.strip_prefix("::add-mask::").unwrap_or("<no prefix>");
            assert!(
                forms.contains(&decoded(data)),
                "{line:?} masks something else"
            );
        }
    }

    /// No form is whitespace only, and a line shorter than the floor is not masked on its own,
    /// though the whole value still is.
    #[test]
    fn whitespace_and_short_lines_are_not_masked_alone() {
        let value = "a\n \n\t\nb\n  long line  ";
        let forms = forms(value);
        assert!(
            forms.iter().all(|form| !form.trim().is_empty()),
            "{forms:?}"
        );
        assert!(forms.contains(value));
        assert!(forms.contains("  long line  "));
        for short in ["a", "b", " ", "\t", "abc"] {
            assert!(
                !forms.contains(short),
                "{short:?} masked alone in {forms:?}"
            );
        }
        assert!(commands(value).iter().all(|line| line != "::add-mask:: "));
        assert!(super::forms("abc\nabcd").contains("abcd"));
        assert!(!super::forms("abc\nabcd").contains("abc"));
        assert!(super::forms("ab").contains("ab"));
        assert!(
            super::forms(" \n\t")
                .iter()
                .all(|form| !form.trim().is_empty())
        );
    }

    /// Every printable form of a secret is among its forms, and none is empty.
    #[test]
    fn every_printable_form_of_a_secret_is_masked() {
        let value = "a+b/c=d\"e\nline two";
        let forms = forms(value);
        let base64 = image::base64(value.as_bytes());
        let base64url = base64.replace('+', "-").replace('/', "_");
        for expected in [
            value.to_owned(),
            base64.clone(),
            base64.trim_end_matches('=').to_owned(),
            base64url.clone(),
            base64url.trim_end_matches('=').to_owned(),
            "a%2Bb%2Fc%3Dd%22e%0Aline%20two".to_owned(),
            "a+b/c=d\\\"e\\nline two".to_owned(),
            "a+b/c=d\"e".to_owned(),
            "line two".to_owned(),
        ] {
            assert!(forms.contains(&expected), "{expected} not in {forms:?}");
        }
        assert!(forms.iter().all(|form| !form.is_empty()));
        assert_eq!(
            super::forms("plain"),
            BTreeSet::from([
                "plain".to_owned(),
                image::base64(b"plain"),
                image::base64(b"plain").trim_end_matches('=').to_owned(),
            ])
        );
    }
}
