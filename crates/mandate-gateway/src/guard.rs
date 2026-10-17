//! The prompt guard (inference spec §8.3, INF-8, E15-10): a deterministic scan of a rendered
//! prompt before any byte leaves. It can be called on its own, with no call, no reservation, and
//! no cost (agent harness spec §10.1 ask 4), and gives the same verdict either way.
//!
//! It refuses, in this order:
//! - **a credential:** a known secret prefix at the start of a word followed by at least
//!   [`MIN_SECRET_TAIL`] key characters with a digit among them; an AWS or Alpaca key id exactly;
//!   key material markers; a JWT; or any run of at least [`MIN_SECRET_RUN`] key characters that
//!   mixes upper case, lower case, and digits (an unprefixed secret, such as an Alpaca or AWS
//!   secret key). Prose such as "risk-adjusted-returns", hashes, which are one case, and filing
//!   paths and hyphenated names, whose slashes and hyphens end a run, are not keys;
//! - **one of the workspace's broker account identifiers;**
//! - **personal data:** a listed value, or an email address (a local part, `@`, and a domain whose
//!   last label is letters), so trade notation such as `100@12.50` is not an address;
//! - **any identifier or value of another workspace.**
//!
//! Listed values are compared with case and every character but letters and digits (of any
//! script) removed from both sides, so `ACCT 7788-12` matches a listed `ACCT-778812`; a value with fewer than
//! [`MIN_LISTED`] letters and digits is never listed, so a stray digit does not refuse every
//! prompt. A false refusal is safe: the model has no fresh output (INF-4).

/// The fewest letters and digits a listed value must have to be matched.
pub const MIN_LISTED: usize = 4;
/// The shortest run of key characters after a credential prefix that counts as a credential.
pub const MIN_SECRET_TAIL: usize = 16;
/// The shortest unprefixed run of mixed-case key characters with a digit that counts as a secret.
pub const MIN_SECRET_RUN: usize = 32;

/// Prefixes that start a provider or platform secret: OpenRouter, Anthropic and other `sk-` keys,
/// Stripe, Hugging Face, Google, GitHub, GitLab, Slack, and bearer tokens.
const SECRET_PREFIXES: [&str; 20] = [
    "sk-or-",
    "sk-ant-",
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "pk_live_",
    "hf_",
    "AIza",
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xapp-",
    "Bearer ",
];
/// Key ids of a fixed shape: the prefix, then exactly this many upper-case letters and digits.
const KEY_IDS: [(&str, usize); 4] = [("AKIA", 16), ("ASIA", 16), ("PK", 18), ("AK", 18)];
/// Markers that alone mean key material.
const KEY_MARKERS: [&str; 2] = ["-----BEGIN", "PRIVATE KEY"];

/// The values the guard compares a prompt against, from the workspace deployment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardContext {
    /// The workspace's broker account identifiers (account numbers and refs).
    pub account_identifiers: Vec<String>,
    /// Personal-data values held for the workspace (journal spec §6.4): names, emails, phones.
    pub personal_data: Vec<String>,
    /// Identifiers and values of every other workspace in the deployment (INF-9).
    pub foreign_values: Vec<String>,
}

/// The rule a refused prompt broke. The record names the rule, never the content (inference spec
/// §9's guard row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardRule {
    Credential,
    AccountIdentifier,
    PersonalData,
    ForeignWorkspace,
    /// The inputs differ from the template's input contract, or one would break out of its data
    /// section (journal spec rule 148's `input_contract`). [`check`] never returns it; the call
    /// path records it when rendering refuses.
    InputContract,
}

impl GuardRule {
    pub fn code(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::AccountIdentifier => "account_identifier",
            Self::PersonalData => "personal_data",
            Self::ForeignWorkspace => "foreign_workspace",
            Self::InputContract => "input_contract",
        }
    }
}

/// The guard's verdict on `prompt`: `Ok` or the first rule it breaks.
pub fn check(prompt: &str, context: &GuardContext) -> Result<(), GuardRule> {
    if has_credential(prompt) {
        return Err(GuardRule::Credential);
    }
    if lists_any(prompt, &context.account_identifiers) {
        return Err(GuardRule::AccountIdentifier);
    }
    if lists_any(prompt, &context.personal_data) || has_email(prompt) {
        return Err(GuardRule::PersonalData);
    }
    if lists_any(prompt, &context.foreign_values) {
        return Err(GuardRule::ForeignWorkspace);
    }
    Ok(())
}

/// Letters and digits of any script only, lower-cased: the form listed values are compared in.
fn folded(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn lists_any(prompt: &str, values: &[String]) -> bool {
    let haystack = folded(prompt);
    values.iter().any(|v| {
        let needle = folded(v);
        needle.chars().count() >= MIN_LISTED && haystack.contains(needle.as_str())
    })
}

fn key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Whether the text before byte `at` ends in a character that could continue a key.
fn inside_a_word(prompt: &str, at: usize) -> bool {
    prompt
        .get(..at)
        .and_then(|before| before.chars().next_back())
        .is_some_and(key_char)
}

fn has_credential(prompt: &str) -> bool {
    KEY_MARKERS.iter().any(|m| prompt.contains(m))
        || has_prefixed_secret(prompt)
        || has_key_id(prompt)
        || has_jwt(prompt)
        || has_mixed_run(prompt)
}

fn has_prefixed_secret(prompt: &str) -> bool {
    SECRET_PREFIXES.iter().any(|prefix| {
        prompt.match_indices(prefix).any(|(at, _)| {
            let tail = prompt.get(at.saturating_add(prefix.len())..).unwrap_or("");
            let run: Vec<char> = tail.chars().take_while(|c| key_char(*c)).collect();
            !inside_a_word(prompt, at)
                && run.len() >= MIN_SECRET_TAIL
                && run.iter().any(char::is_ascii_digit)
        })
    })
}

/// An AWS or Alpaca key id: its prefix at the start of a word, then exactly the shape's count of
/// upper-case letters and digits, a digit among them, and no further key character.
fn has_key_id(prompt: &str) -> bool {
    KEY_IDS.iter().any(|(prefix, length)| {
        prompt.match_indices(prefix).any(|(at, _)| {
            let tail = prompt.get(at.saturating_add(prefix.len())..).unwrap_or("");
            let run: Vec<char> = tail.chars().take_while(|c| key_char(*c)).collect();
            let shaped = run
                .iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            !inside_a_word(prompt, at)
                && run.len() == *length
                && shaped
                && run.iter().any(char::is_ascii_digit)
        })
    })
}

/// A run of at least [`MIN_SECRET_RUN`] letters, digits, `_`, `+`, and `=` holding an upper-case
/// letter, a lower-case letter, and a digit. A hyphen ends a run, so hyphenated names such as XBRL
/// concepts are words, not secrets.
fn has_mixed_run(prompt: &str) -> bool {
    prompt
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=')))
        .any(|run| {
            run.chars().count() >= MIN_SECRET_RUN
                && run.chars().any(|c| c.is_ascii_uppercase())
                && run.chars().any(|c| c.is_ascii_lowercase())
                && run.chars().any(|c| c.is_ascii_digit())
        })
}

/// Three dot-separated base64url runs whose first starts `eyJ`, the encoded `{"`.
fn has_jwt(prompt: &str) -> bool {
    prompt.match_indices("eyJ").any(|(at, _)| {
        let rest = prompt.get(at..).unwrap_or("");
        let token: String = rest
            .chars()
            .take_while(|c| key_char(*c) || *c == '.')
            .collect();
        let parts: Vec<&str> = token.split('.').collect();
        parts.len() >= 3 && parts.iter().take(3).all(|p| p.len() >= 8)
    })
}

/// A local part, `@`, and a domain of at least two labels whose host has a letter and whose last
/// label is two or more letters.
fn has_email(prompt: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+');
    prompt.match_indices('@').any(|(at, _)| {
        let before = prompt.get(..at).unwrap_or("");
        let after = prompt.get(at.saturating_add(1)..).unwrap_or("");
        let local = before.chars().rev().take_while(|c| word(*c)).count();
        let domain: String = after.chars().take_while(|c| word(*c)).collect();
        let domain = domain.trim_end_matches('.');
        local > 0
            && domain.rsplit_once('.').is_some_and(|(host, tld)| {
                host.chars().any(|c| c.is_ascii_alphabetic())
                    && tld.len() >= 2
                    && tld.chars().all(|c| c.is_ascii_alphabetic())
            })
    })
}
