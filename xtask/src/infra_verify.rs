//! Whether staging is up (E21-58, DEC-2050): the read-only check staging's apply runs after lane
//! GO's bootstrap, with the same short-lived kubeconfig and inside the same control-plane window
//! (DEC-2028), so it answers "is staging up?" without an agent touching the cluster. It reads
//! Flux's objects, the platform's and the workspaces' pods, the kill-switch admission bindings and
//! the signature webhook, the tunnel's health from Cloudflare, and the API's answer through
//! Access, and prints each as an `[ok]` or `[MISSING]` line, as the preflight does. It polls until
//! every line holds or its rounds run out, and only reads: no `kubectl` verb but `get`, no request
//! but `GET`.

use anyhow::{Result, bail};
use serde_json::Value;

use crate::infra_preflight::Checklist;
use crate::infra_window::Http;

/// Cloudflare's API, as the preflight calls it.
const CF_API: &str = "https://api.cloudflare.com/client/v4";
/// The namespaces the platform runs in, each of which must hold a pod; a workspace's (`ws-*`) is
/// checked too, but may hold none before its first promotion (DEC-2016).
const PLATFORM_NAMESPACES: [&str; 3] = ["flux-system", "cnpg-system", "cosign-system"];
/// Flux's objects, by kind, that must each be Ready, as one `kubectl get` names them.
const FLUX_KINDS: [(&str, &str); 4] = [
    ("GitRepository", "gitrepositories.source.toolkit.fluxcd.io"),
    ("OCIRepository", "ocirepositories.source.toolkit.fluxcd.io"),
    (
        "Kustomization",
        "kustomizations.kustomize.toolkit.fluxcd.io",
    ),
    ("HelmRelease", "helmreleases.helm.toolkit.fluxcd.io"),
];
/// The kill-switch admission policies' bindings (DEC-2045, DEC-2046).
const BINDINGS: [&str; 2] = ["mandate-control-pods", "mandate-control-jobs"];
/// The signature webhooks' configuration, as `admission.yaml`'s patch names it (DEC-2016 item 1).
const SIGNATURE_WEBHOOK: &str = "policy.sigstore.dev";
/// How many times the check reads staging before it reports what is still missing: with the
/// HTTP layer's five-second pause, eight minutes, inside the kubeconfig's fifteen.
pub(crate) const ROUNDS: u32 = 96;

/// `kubectl get` against the cluster, its arguments after `get`, answering what it printed.
pub(crate) type Kubectl<'a> = &'a dyn Fn(&[&str]) -> Result<String>;

/// Where staging's edge is, from its root's outputs. The account is a sensitive output, masked
/// before it gets here, and is never printed.
pub(crate) struct Edge<'a> {
    pub(crate) cloudflare_token: &'a str,
    pub(crate) account: &'a str,
    pub(crate) tunnel_id: &'a str,
    pub(crate) api_hostname: &'a str,
}

/// One reading of staging, as the checklist it prints.
fn checks(_kubectl: Kubectl<'_>, _http: &dyn Http, _edge: &Edge<'_>) -> Result<Checklist> {
    bail!("pending DEC-2050: staging's verify is not written yet")
}

/// Reads staging until every line holds, or [`ROUNDS`] times, pausing between readings, and
/// answers the last reading.
pub(crate) fn verify(kubectl: Kubectl<'_>, http: &dyn Http, edge: &Edge<'_>) -> Result<Checklist> {
    let mut round = 1;
    loop {
        let list = checks(kubectl, http, edge)?;
        if list.complete() || round >= ROUNDS {
            return Ok(list);
        }
        http.pause();
        round += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use anyhow::anyhow;
    use serde_json::json;

    use super::*;
    use crate::infra_preflight::Answer;

    const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";

    fn edge() -> Edge<'static> {
        Edge {
            cloudflare_token: "cf-token",
            account: ACCOUNT,
            tunnel_id: "tunnel-1",
            api_hostname: "api.staging.example.com",
        }
    }

    fn ready(kind: &str, namespace: &str, name: &str, holds: bool) -> Value {
        json!({
            "kind": kind,
            "metadata": {"name": name, "namespace": namespace},
            "status": {"conditions": [{"type": "Ready", "status": if holds { "True" } else { "False" }}]},
        })
    }

    fn pod(namespace: &str, name: &str, phase: &str, containers: &[bool]) -> Value {
        json!({
            "metadata": {"name": name, "namespace": namespace},
            "status": {
                "phase": phase,
                "containerStatuses": containers.iter().map(|ready| json!({"ready": ready})).collect::<Vec<_>>(),
            },
        })
    }

    /// A staging that is up, which each test breaks one way.
    struct Staging {
        flux: Vec<Value>,
        pods: Vec<Value>,
        bindings: Vec<Value>,
        webhooks: Vec<Value>,
        tunnel: &'static str,
        api: u16,
        unreadable: Option<&'static str>,
        rounds_until_up: u32,
        reads: Cell<u32>,
        pauses: Cell<u32>,
        urls: std::cell::RefCell<Vec<String>>,
    }

    impl Staging {
        fn up() -> Self {
            Self {
                flux: vec![
                    ready("GitRepository", "flux-system", "mandate", true),
                    ready("OCIRepository", "flux-system", "mandate-workspace", true),
                    ready("Kustomization", "flux-system", "mandate", true),
                    ready("HelmRelease", "flux-system", "workspace", true),
                ],
                pods: vec![
                    pod("flux-system", "source-controller-x", "Running", &[true]),
                    pod("cnpg-system", "cnpg-x", "Running", &[true]),
                    pod("cosign-system", "webhook-x", "Running", &[true]),
                    pod("ws-ws1", "journal-1", "Running", &[true, true]),
                    pod("ws-ws1", "migrate-x", "Succeeded", &[false]),
                    pod("kube-system", "coredns-x", "Pending", &[false]),
                ],
                bindings: BINDINGS
                    .iter()
                    .map(|name| {
                        json!({"metadata": {"name": name}, "spec": {"policyName": name, "validationActions": ["Deny"]}})
                    })
                    .collect(),
                webhooks: vec![json!({
                    "metadata": {"name": SIGNATURE_WEBHOOK},
                    "webhooks": [{"failurePolicy": "Fail"}, {"failurePolicy": "Fail"}],
                })],
                tunnel: "healthy",
                api: 302,
                unreadable: None,
                rounds_until_up: 0,
                reads: Cell::new(0),
                pauses: Cell::new(0),
                urls: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn kubectl(&self, args: &[&str]) -> Result<String> {
            assert!(
                !args.iter().any(|arg| matches!(
                    *arg,
                    "apply"
                        | "delete"
                        | "patch"
                        | "create"
                        | "edit"
                        | "label"
                        | "annotate"
                        | "scale"
                        | "exec"
                )),
                "verify only reads: {args:?}"
            );
            let what = args.first().copied().unwrap_or_default();
            if what == "pods" {
                self.reads.set(self.reads.get() + 1);
            }
            if self.unreadable == Some(what) {
                return Err(anyhow!("the server could not be reached"));
            }
            let late = self.reads.get() <= self.rounds_until_up;
            let items = match what {
                "pods" if late => vec![pod("cnpg-system", "cnpg-x", "Pending", &[false])],
                "pods" => self.pods.clone(),
                "validatingadmissionpolicybindings" => self.bindings.clone(),
                "validatingwebhookconfigurations" => self.webhooks.clone(),
                kinds if kinds.contains("fluxcd.io") => self.flux.clone(),
                other => return Err(anyhow!("unexpected kubectl get {other}")),
            };
            Ok(json!({"items": items}).to_string())
        }
    }

    impl Http for Staging {
        fn api(&self, method: &str, url: &str, token: &str, body: Option<&str>) -> Result<Answer> {
            assert_eq!(
                (method, token, body),
                ("GET", "cf-token", None),
                "verify only reads"
            );
            self.urls.borrow_mut().push(url.to_owned());
            Ok(Answer {
                status: 200,
                body: json!({"success": true, "result": {"status": self.tunnel}}).to_string(),
            })
        }

        fn get_public(&self, url: &str) -> Result<Answer> {
            self.urls.borrow_mut().push(url.to_owned());
            Ok(Answer {
                status: self.api,
                body: String::new(),
            })
        }

        fn pause(&self) {
            self.pauses.set(self.pauses.get() + 1);
        }

        fn now(&self) -> Result<i64> {
            Ok(0)
        }
    }

    fn run(staging: &Staging) -> Result<String> {
        let list = verify(&|args| staging.kubectl(args), staging, &edge())?;
        Ok(list.render_titled("infra verify (staging)"))
    }

    fn missing(text: &str) -> Vec<&str> {
        text.lines()
            .filter_map(|line| line.trim_start().strip_prefix("[MISSING] "))
            .collect()
    }

    /// A staging that is up holds on the first reading, reading Cloudflare's record of the tunnel
    /// and the API's public hostname, and printing neither the account nor the token.
    #[test]
    fn an_up_staging_holds_at_once() -> Result<()> {
        let staging = Staging::up();
        let text = run(&staging)?;
        assert!(text.starts_with("infra verify (staging):\n"), "{text}");
        assert_eq!(missing(&text), Vec::<&str>::new(), "{text}");
        assert_eq!(staging.pauses.get(), 0);
        assert_eq!(
            *staging.urls.borrow(),
            [
                format!("{CF_API}/accounts/{ACCOUNT}/cfd_tunnel/tunnel-1"),
                "https://api.staging.example.com/".to_owned(),
            ]
        );
        assert!(
            !text.contains(ACCOUNT) && !text.contains("cf-token"),
            "{text}"
        );
        for line in [
            "HelmRelease flux-system/workspace is Ready",
            "pod cnpg-system/cnpg-x is Running and Ready",
            "binding mandate-control-jobs denies",
            "webhook policy.sigstore.dev fails closed",
            "the tunnel is healthy",
            "the API answers only through Access (302 without credentials)",
        ] {
            assert!(
                text.contains(&format!("[ok]      {line}")),
                "{line}: {text}"
            );
        }
        assert!(
            !text.contains("kube-system") && !text.contains("migrate-x"),
            "{text}"
        );
        Ok(())
    }

    /// Each way staging can be down is its own MISSING line, after every round is spent.
    #[test]
    fn each_fault_is_named() -> Result<()> {
        let faults: [(fn(&mut Staging), &str); 11] = [
            (
                |s| s.flux[3] = ready("HelmRelease", "flux-system", "workspace", false),
                "HelmRelease flux-system/workspace is Ready",
            ),
            (
                |s| {
                    s.flux.remove(1);
                },
                "Flux has an OCIRepository",
            ),
            (
                |s| s.pods[3] = pod("ws-ws1", "journal-1", "Running", &[true, false]),
                "pod ws-ws1/journal-1 is Running and Ready",
            ),
            (
                |s| s.pods[1] = pod("cnpg-system", "cnpg-x", "Failed", &[false]),
                "pod cnpg-system/cnpg-x is Running and Ready",
            ),
            (
                |s| {
                    s.pods.remove(2);
                },
                "cosign-system runs a pod",
            ),
            (
                |s| s.bindings[1]["spec"]["validationActions"] = json!(["Warn"]),
                "binding mandate-control-jobs denies",
            ),
            (
                |s| {
                    s.bindings.remove(0);
                },
                "binding mandate-control-pods denies",
            ),
            (
                |s| s.webhooks[0]["webhooks"][1]["failurePolicy"] = json!("Ignore"),
                "webhook policy.sigstore.dev fails closed",
            ),
            (
                |s| s.tunnel = "down",
                "the tunnel is healthy (Cloudflare reports down)",
            ),
            (
                |s| s.api = 200,
                "the API answers only through Access (200 without credentials)",
            ),
            (
                |s| s.api = 502,
                "the API answers only through Access (502 without credentials)",
            ),
        ];
        for (fault, line) in faults {
            let mut staging = Staging::up();
            fault(&mut staging);
            let text = run(&staging)?;
            assert_eq!(missing(&text), [line], "{text}");
            assert_eq!(staging.pauses.get(), ROUNDS - 1, "{line}");
        }
        Ok(())
    }

    /// A staging still reconciling is read again until it is up; one kubectl cannot read is a
    /// MISSING line, never an abort.
    #[test]
    fn it_waits_for_staging_and_names_what_it_cannot_read() -> Result<()> {
        let mut staging = Staging::up();
        staging.rounds_until_up = 3;
        let text = run(&staging)?;
        assert_eq!(missing(&text), Vec::<&str>::new(), "{text}");
        assert_eq!(staging.pauses.get(), 3);
        let mut staging = Staging::up();
        staging.unreadable = Some("validatingwebhookconfigurations");
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            ["kubectl can read validatingwebhookconfigurations"],
            "{text}"
        );
        Ok(())
    }
}
