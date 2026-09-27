//! The host controls (task brief, "The non-paper refusal", TI-5).
//!
//! The shell cannot reach a host of its own: it names no HTTP or TLS crate, it holds no host
//! literal outside `#[cfg(test)]`, and every URL is built inside `mandate-alpaca` as
//! `format!("{PAPER_HOST}{path}")`. What remains to check is an **input**: there is no `--host`
//! argument, no host variable, and no configuration file, so an operator who tries to point the
//! tracer elsewhere through the environment gets a stop rather than a silent redirect.

use crate::error::ShellError;

/// Refuses when any variable whose name mentions Alpaca holds something that looks like a URL or a
/// host. The error names the variable only, never its value, which may be a credential (TI-8).
///
/// # Errors
/// [`ShellError::NonPaperHost`] naming the first such variable.
pub fn refuse_configured_host<I, K, V>(vars: I) -> Result<(), ShellError>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    for (name, value) in vars {
        let name = name.as_ref();
        if name.to_ascii_uppercase().contains("ALPACA") && looks_like_a_host(value.as_ref()) {
            return Err(ShellError::NonPaperHost {
                var: name.to_owned(),
            });
        }
    }
    Ok(())
}

/// A scheme, or a dotted name ending in a domain Alpaca serves, whatever it is prefixed with.
fn looks_like_a_host(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.contains("://") || value.starts_with("http") || value.contains(".markets")
}

#[cfg(test)]
mod tests {
    use super::refuse_configured_host;
    use crate::error::ShellError;

    fn refused_var(vars: &[(&str, &str)]) -> Option<String> {
        match refuse_configured_host(vars.iter().copied()) {
            Err(ShellError::NonPaperHost { var }) => Some(var),
            Ok(()) | Err(_) => None,
        }
    }

    #[test]
    fn refuses_configured_host() {
        for value in [
            "https://api.alpaca.markets",
            "https://paper-api.alpaca.markets.evil.example",
            "http://127.0.0.1:8080",
            "paper-api.alpaca.markets",
            "  HTTPS://API.ALPACA.MARKETS  ",
        ] {
            assert_eq!(
                refused_var(&[("PATH", "/usr/bin"), ("ALPACA_HOST", value)]).as_deref(),
                Some("ALPACA_HOST"),
                "{value}"
            );
        }
        assert_eq!(
            refused_var(&[("apca_alpaca_base_url", "https://api.alpaca.markets")]).as_deref(),
            Some("apca_alpaca_base_url")
        );
    }

    #[test]
    fn a_refusal_names_the_variable_and_never_its_value() {
        let secret = "https://sentinel-credential-value.alpaca.markets";
        let error = refuse_configured_host([("MANDATE_ALPACA_PAPER_SECRET", secret)]);
        let shown = match error {
            Err(e) => format!("{e} {e:?} {}", e.code()),
            Ok(()) => String::new(),
        };
        assert!(shown.contains("MANDATE_ALPACA_PAPER_SECRET"), "{shown}");
        assert!(shown.contains("non_paper_host"), "{shown}");
        assert!(!shown.contains("sentinel-credential-value"), "{shown}");
    }

    #[test]
    fn a_key_or_an_unrelated_url_is_not_a_configured_host() {
        assert!(
            refuse_configured_host([
                ("MANDATE_ALPACA_PAPER_KEY_ID", "PKSENTINELKEYID"),
                ("MANDATE_ALPACA_PAPER_SECRET", "sentinel-secret"),
                ("HTTPS_PROXY", "http://proxy.internal:3128"),
            ])
            .is_ok()
        );
    }
}
