//! How `cargo xtask cell demo` runs `web/` (lane W2, DEC-2406): on its own fixtures, or against the
//! cell's API as the demo owner once sign-in through the stub issuer is ready. The scenario's web
//! gestures (`scenario.rs`, lane E2E) drive the app this starts.

use super::{DEMO_APP_ORIGIN, DEMO_SUBJECT, DEMO_WORKSPACE, LOCAL_ISSUER, WEB_URL};

/// The local API as `web/` reaches it: `localhost`, so its `SameSite=Strict` cookie is same-site
/// with the app origin [`DEMO_APP_ORIGIN`] (DEC-2406).
const WEB_API_ORIGIN: &str = "http://localhost:8787";

/// How `cargo xtask cell demo` runs `web/`: its npm script, where it serves, and the server-side
/// settings it reads (`web/README.md`). Without sign-in it runs on its own fixtures at
/// [`WEB_URL`]; with sign-in it reads the cell's API as the demo owner, through the stub issuer
/// under `next dev` (DEC-2406), at the API's https app origin.
#[derive(Debug, PartialEq, Eq)]
pub struct WebRun {
    pub script: &'static str,
    pub url: &'static str,
    pub env: Vec<(&'static str, String)>,
}

pub fn web_run(signed_in: bool) -> WebRun {
    if !signed_in {
        return WebRun {
            script: "dev",
            url: WEB_URL,
            env: Vec::new(),
        };
    }
    WebRun {
        script: "dev:cell",
        url: DEMO_APP_ORIGIN,
        env: vec![
            ("OWLHEAD_DATA_SOURCE", "api".to_string()),
            ("OWLHEAD_API_ORIGIN", WEB_API_ORIGIN.to_string()),
            ("OWLHEAD_WORKSPACE_ID", DEMO_WORKSPACE.to_string()),
            ("OWLHEAD_STUB_ISSUER", LOCAL_ISSUER.to_string()),
            ("OWLHEAD_STUB_SUBJECT", DEMO_SUBJECT.to_string()),
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use anyhow::{Context, Result};
    use serde_json::Value;

    use super::super::{API_LOCAL_PORT, STUB_PORT};
    use super::*;

    #[test]
    fn web_reads_the_cells_api_as_the_demo_owner_only_once_sign_in_is_ready() -> Result<()> {
        let fixtures = web_run(false);
        assert_eq!(
            (fixtures.script, fixtures.url, fixtures.env.len()),
            ("dev", WEB_URL, 0),
            "without sign-in web/ runs on its own fixtures"
        );
        let cell = web_run(true);
        assert_eq!(
            cell.url, DEMO_APP_ORIGIN,
            "web/ serves at the one origin the API's CORS and CSRF checks admit"
        );
        let env: BTreeMap<&str, &str> = cell.env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            env,
            BTreeMap::from([
                ("OWLHEAD_DATA_SOURCE", "api"),
                ("OWLHEAD_API_ORIGIN", "http://localhost:8787"),
                ("OWLHEAD_WORKSPACE_ID", DEMO_WORKSPACE),
                ("OWLHEAD_STUB_ISSUER", LOCAL_ISSUER),
                ("OWLHEAD_STUB_SUBJECT", DEMO_SUBJECT),
            ]),
            "web/'s settings name the forwarded API, the demo workspace, and the stub's issuer and subject"
        );
        assert!(
            WEB_API_ORIGIN.ends_with(&format!(":{API_LOCAL_PORT}")),
            "web/ reaches the API on the port the demo forwards"
        );
        assert_eq!(
            LOCAL_ISSUER,
            format!("http://127.0.0.1:{STUB_PORT}"),
            "the stub's issuer is the address its forward listens on, so its Host check passes"
        );
        let package: Value = serde_json::from_str(&fs::read_to_string(
            crate::repo_root()?.join("web/package.json"),
        )?)?;
        let origin = DEMO_APP_ORIGIN
            .strip_prefix("https://")
            .and_then(|o| o.split_once(':'))
            .context("the app origin has a host and a port")?;
        assert_eq!(
            package["scripts"][cell.script].as_str(),
            Some(
                format!(
                    "next dev --experimental-https --hostname {} --port {}",
                    origin.0, origin.1
                )
                .as_str()
            ),
            "web/'s script serves https at the app origin"
        );
        Ok(())
    }
}
