//! Staging's stalled HelmReleases restarted by the apply, inside its window: helm-controller gives up
//! on a release once its retries are spent (`Stalled`, reason `RetriesExceeded`) and tries again
//! only when the release's values or chart change, so a fix that lands elsewhere, such as an
//! admission policy, would leave the release stalled until the next promotion. The apply annotates
//! each stalled release as `flux reconcile --reset` does, `reconcile.fluxcd.io/requestedAt` and
//! `reconcile.fluxcd.io/resetAt` with the run's own token, so helm-controller clears its failure
//! counts and runs the release again before verify reads it. A release that is not stalled is left
//! alone, and no person runs a command against the cluster (DEC-2114's rule).

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use crate::vault_init::Kubectl;

/// The annotation that asks helm-controller to reconcile now.
const REQUESTED_AT: &str = "reconcile.fluxcd.io/requestedAt";
/// The annotation that, with a value it has not handled, clears a release's failure counts.
const RESET_AT: &str = "reconcile.fluxcd.io/resetAt";

/// Annotates every HelmRelease whose `Stalled` condition is `True` with `token` as both
/// [`REQUESTED_AT`] and [`RESET_AT`], and returns one word per release reset, by namespace and
/// name alone, or that none was stalled.
///
/// # Errors
/// When the releases cannot be listed or read, or an annotation is refused.
pub(crate) fn reset_stalled(kubectl: Kubectl<'_>, token: &str) -> Result<Vec<String>> {
    let (listed, text) = kubectl(&["get", "helmreleases", "-A", "-o", "json"], None)?;
    ensure!(listed, "listing the HelmReleases failed");
    let listing: Value = serde_json::from_str(&text).context("the HelmRelease list is not JSON")?;
    let items = listing["items"]
        .as_array()
        .context("the HelmRelease list has no items")?;
    let mut words = Vec::new();
    for release in items.iter().filter(|release| stalled(release)) {
        let namespace = release["metadata"]["namespace"]
            .as_str()
            .unwrap_or_default();
        let name = release["metadata"]["name"].as_str().unwrap_or_default();
        ensure!(
            !namespace.is_empty() && !name.is_empty(),
            "a stalled HelmRelease has no namespace or name"
        );
        let requested = format!("{REQUESTED_AT}={token}");
        let reset = format!("{RESET_AT}={token}");
        let (annotated, _) = kubectl(
            &[
                "annotate",
                "helmrelease",
                "-n",
                namespace,
                name,
                "--overwrite",
                &requested,
                &reset,
            ],
            None,
        )?;
        ensure!(annotated, "resetting HelmRelease {namespace}/{name} failed");
        words.push(format!("reset the stalled HelmRelease {namespace}/{name}"));
    }
    if words.is_empty() {
        words.push("no HelmRelease is stalled".to_owned());
    }
    Ok(words)
}

/// Whether a release's `Stalled` condition is `True`.
fn stalled(release: &Value) -> bool {
    release["status"]["conditions"]
        .as_array()
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|c| c["type"] == "Stalled" && c["status"] == "True")
        })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use anyhow::Result;
    use serde_json::json;

    use super::reset_stalled;

    fn release(namespace: &str, name: &str, stalled: &str) -> serde_json::Value {
        json!({
            "metadata": {"namespace": namespace, "name": name},
            "status": {"conditions": [
                {"type": "Ready", "status": "False"},
                {"type": "Stalled", "status": stalled, "reason": "RetriesExceeded"},
            ]},
        })
    }

    fn run(items: serde_json::Value, annotate_ok: bool) -> (Result<Vec<String>>, Vec<Vec<String>>) {
        let calls = RefCell::new(Vec::new());
        let listing = json!({"items": items}).to_string();
        let kubectl = |args: &[&str], stdin: Option<&str>| -> Result<(bool, String)> {
            assert!(stdin.is_none(), "a reset writes nothing on standard input");
            calls
                .borrow_mut()
                .push(args.iter().map(|arg| (*arg).to_owned()).collect());
            if args.first() == Some(&"get") {
                Ok((true, listing.clone()))
            } else {
                Ok((annotate_ok, String::new()))
            }
        };
        let words = reset_stalled(&kubectl, "infra-42");
        (words, calls.into_inner())
    }

    #[test]
    fn only_a_stalled_release_is_reset_as_flux_reconcile_reset_does() -> Result<()> {
        let (words, calls) = run(
            json!([
                release("flux-system", "workspace", "True"),
                release("flux-system", "journal", "False"),
                {"metadata": {"namespace": "flux-system", "name": "vault"}, "status": {}},
            ]),
            true,
        );
        assert_eq!(
            words?,
            ["reset the stalled HelmRelease flux-system/workspace"]
        );
        assert_eq!(
            calls,
            [
                vec!["get", "helmreleases", "-A", "-o", "json"],
                vec![
                    "annotate",
                    "helmrelease",
                    "-n",
                    "flux-system",
                    "workspace",
                    "--overwrite",
                    "reconcile.fluxcd.io/requestedAt=infra-42",
                    "reconcile.fluxcd.io/resetAt=infra-42",
                ],
            ]
        );
        Ok(())
    }

    #[test]
    fn nothing_stalled_writes_nothing_and_says_so() -> Result<()> {
        let (words, calls) = run(json!([release("flux-system", "workspace", "False")]), true);
        assert_eq!(words?, ["no HelmRelease is stalled"]);
        assert_eq!(calls.len(), 1, "only the list is read");
        Ok(())
    }

    #[test]
    fn a_refused_annotation_fails_the_apply() {
        let (words, _) = run(json!([release("flux-system", "workspace", "True")]), false);
        assert!(
            words.is_err_and(|e| e.to_string().contains("flux-system/workspace")),
            "a reset the cluster refuses is an error naming the release"
        );
    }
}
