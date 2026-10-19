//! Whether staging is up (E21-58, DEC-2050): the read-only check staging's apply runs after lane
//! GO's bootstrap, with the same short-lived kubeconfig and inside the same control-plane window
//! (DEC-2028), so it answers "is staging up?" without an agent touching the cluster. It reads
//! Flux's objects, the platform's and the workspaces' pods, the kill-switch admission bindings and
//! the signature webhook, the tunnel's health from Cloudflare, and the API's answer through
//! Access, and prints each as an `[ok]` or `[MISSING]` line, as the preflight does. It polls until
//! every line holds or its rounds run out, and only reads: no `kubectl` verb but `get`, no request
//! but `GET`.

use anyhow::Result;
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
/// The vault's chart, as its pods' `app.kubernetes.io/name` label names it (DEC-2051). Its pod is
/// ready only once unsealed; staging's apply initializes it just before this check (DEC-2052), so a
/// vault still not ready is a failure like any other.
const VAULT_CHART: &str = "mandate-vault-server";
/// The kill-switch admission policies' bindings (DEC-2045, DEC-2046).
const BINDINGS: [&str; 2] = ["mandate-control-pods", "mandate-control-jobs"];
/// The signature webhooks' configuration, as `admission.yaml`'s patch names it (DEC-2016 item 1).
const SIGNATURE_WEBHOOK: &str = "policy.sigstore.dev";
/// How long the check reads staging before it reports what is still missing, in seconds by the
/// HTTP layer's clock: eight minutes, inside the kubeconfig's fifteen, however long each reading
/// takes.
pub(crate) const SECONDS: i64 = 480;

/// The change the edge's lines wait on: until the API pod's tunnel connector lands, an unhealthy
/// tunnel or an API Access cannot reach is a pending note, not a failure (DEC-2050 item 4). The PR
/// that lands the connector sets this to `None`. An API that answers without Access fails always.
const EDGE_PENDING: Option<&str> = Some("#1539");

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

/// The items `kubectl get` lists for `args`, or a MISSING line naming `what` it could not read.
fn listed(
    list: &mut Checklist,
    kubectl: Kubectl<'_>,
    what: &str,
    args: &[&str],
) -> Option<Vec<Value>> {
    let items = kubectl(args)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|listing| listing.get("items").and_then(Value::as_array).cloned());
    if items.is_none() {
        list.check(false, format!("kubectl can read {what}"));
    }
    items
}

fn text<'v>(value: &'v Value, path: &[&str]) -> &'v str {
    path.iter()
        .fold(value, |node, key| &node[*key])
        .as_str()
        .unwrap_or_default()
}

/// Whether an object's `Ready` condition is `True`, as Flux sets it.
fn is_ready(object: &Value) -> bool {
    object["status"]["conditions"]
        .as_array()
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|c| text(c, &["type"]) == "Ready" && text(c, &["status"]) == "True")
        })
}

fn where_is(object: &Value) -> String {
    format!(
        "{}/{}",
        text(object, &["metadata", "namespace"]),
        text(object, &["metadata", "name"])
    )
}

/// Flux's sources, Kustomizations and HelmReleases: at least one of each kind, each Ready.
fn flux(list: &mut Checklist, kubectl: Kubectl<'_>) {
    let kinds: Vec<&str> = FLUX_KINDS.iter().map(|(_, resource)| *resource).collect();
    let Some(items) = listed(
        list,
        kubectl,
        "Flux's objects",
        &[&kinds.join(","), "-A", "-o", "json"],
    ) else {
        return;
    };
    for (kind, _) in FLUX_KINDS {
        let of_kind: Vec<&Value> = items
            .iter()
            .filter(|o| text(o, &["kind"]) == kind)
            .collect();
        if of_kind.is_empty() {
            let article = if kind.starts_with(['A', 'E', 'I', 'O', 'U']) {
                "an"
            } else {
                "a"
            };
            list.check(false, format!("Flux has {article} {kind}"));
        }
        for object in of_kind {
            list.check(
                is_ready(object),
                format!("{kind} {} is Ready", where_is(object)),
            );
        }
    }
}

/// Every pod of the platform and the workspaces, but a finished Job's, Running with every
/// container ready, and at least one in each platform namespace.
fn pods(list: &mut Checklist, kubectl: Kubectl<'_>) {
    let Some(items) = listed(list, kubectl, "pods", &["pods", "-A", "-o", "json"]) else {
        return;
    };
    let ours = |pod: &&Value| {
        let namespace = text(pod, &["metadata", "namespace"]);
        PLATFORM_NAMESPACES.contains(&namespace) || namespace.starts_with("ws-")
    };
    for pod in items.iter().filter(ours) {
        if text(pod, &["status", "phase"]) == "Succeeded" {
            continue;
        }
        let containers = pod["status"]["containerStatuses"].as_array();
        let up = text(pod, &["status", "phase"]) == "Running"
            && containers.is_some_and(|all| {
                !all.is_empty() && all.iter().all(|c| c["ready"].as_bool() == Some(true))
            });
        let mut what = format!("pod {} is Running and Ready", where_is(pod));
        if text(pod, &["metadata", "labels", "app.kubernetes.io/name"]) == VAULT_CHART {
            what.push_str(", so unsealed");
        }
        list.check(up, what);
    }
    for namespace in PLATFORM_NAMESPACES {
        if !items
            .iter()
            .any(|pod| text(pod, &["metadata", "namespace"]) == namespace)
        {
            list.check(false, format!("{namespace} runs a pod"));
        }
    }
}

/// The kill-switch policies' bindings deny, and the signature webhooks fail closed.
fn admission(list: &mut Checklist, kubectl: Kubectl<'_>) {
    let bindings = "validatingadmissionpolicybindings";
    if let Some(items) = listed(list, kubectl, bindings, &[bindings, "-o", "json"]) {
        for name in BINDINGS {
            let denies = items.iter().any(|binding| {
                text(binding, &["metadata", "name"]) == name
                    && text(binding, &["spec", "policyName"]) == name
                    && binding["spec"]["validationActions"] == serde_json::json!(["Deny"])
            });
            list.check(denies, format!("binding {name} denies"));
        }
    }
    let webhooks = "validatingwebhookconfigurations";
    if let Some(items) = listed(list, kubectl, webhooks, &[webhooks, "-o", "json"]) {
        let closed = items.iter().any(|configuration| {
            text(configuration, &["metadata", "name"]) == SIGNATURE_WEBHOOK
                && configuration["webhooks"].as_array().is_some_and(|all| {
                    !all.is_empty() && all.iter().all(|w| text(w, &["failurePolicy"]) == "Fail")
                })
        });
        list.check(closed, format!("webhook {SIGNATURE_WEBHOOK} fails closed"));
    }
}

/// A line of the edge's: failing, or pending on `pending` when it names a change.
fn edge_line(
    list: &mut Checklist,
    pending: Option<&'static str>,
    holds: bool,
    what: impl Into<String>,
) {
    match pending {
        Some(until) => list.check_or_pending(holds, until, what),
        None => list.check(holds, what),
    }
}

/// The tunnel's health as Cloudflare records it, and the API's answer to a request without
/// credentials: Access redirects it to its login or refuses it; a success means the API is open.
fn edge_checks(
    list: &mut Checklist,
    http: &dyn Http,
    edge: &Edge<'_>,
    pending: Option<&'static str>,
) {
    let url = format!(
        "{CF_API}/accounts/{}/cfd_tunnel/{}",
        edge.account, edge.tunnel_id
    );
    let status = http
        .api("GET", &url, edge.cloudflare_token, None)
        .ok()
        .filter(|answer| answer.ok())
        .and_then(|answer| serde_json::from_str::<Value>(&answer.body).ok())
        .map(|record| text(&record, &["result", "status"]).to_owned());
    match status.as_deref() {
        Some("healthy") => edge_line(list, pending, true, "the tunnel is healthy".to_owned()),
        Some(other) => {
            let said: String = other
                .chars()
                .filter(char::is_ascii_alphabetic)
                .take(16)
                .collect();
            edge_line(
                list,
                pending,
                false,
                format!("the tunnel is healthy (Cloudflare reports {said})"),
            );
        }
        None => edge_line(
            list,
            pending,
            false,
            "the tunnel is healthy (Cloudflare did not answer)".to_owned(),
        ),
    }
    let line = |answer: &str| {
        format!("the API answers only through Access ({answer} without credentials)")
    };
    match http.get_public(&format!("https://{}/", edge.api_hostname)) {
        Ok(answer) if (200..300).contains(&answer.status) => {
            list.check(false, line(&answer.status.to_string()));
        }
        Ok(answer) => edge_line(
            list,
            pending,
            (300..400).contains(&answer.status) || matches!(answer.status, 401 | 403),
            line(&answer.status.to_string()),
        ),
        Err(_) => edge_line(list, pending, false, line("no answer")),
    }
}

/// One reading of staging, as the checklist it prints.
fn checks(
    kubectl: Kubectl<'_>,
    http: &dyn Http,
    edge: &Edge<'_>,
    pending: Option<&'static str>,
) -> Result<Checklist> {
    let mut list = Checklist::default();
    flux(&mut list, kubectl);
    pods(&mut list, kubectl);
    admission(&mut list, kubectl);
    edge_checks(&mut list, http, edge, pending);
    Ok(list)
}

/// Reads staging until every line holds or [`SECONDS`] have passed since the first reading,
/// pausing between readings, and answers the last reading.
pub(crate) fn verify(kubectl: Kubectl<'_>, http: &dyn Http, edge: &Edge<'_>) -> Result<Checklist> {
    let started = http.now()?;
    loop {
        let list = checks(kubectl, http, edge, EDGE_PENDING)?;
        if list.complete() || http.now()? - started >= SECONDS {
            return Ok(list);
        }
        http.pause();
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

    /// One way to break an up staging.
    type Fault = fn(&mut Staging);

    fn vault(ready: bool) -> Value {
        let mut found = pod("ws-ws1", "vault-0", "Running", &[ready]);
        found["metadata"]["labels"] = json!({"app.kubernetes.io/name": VAULT_CHART});
        found
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
        seconds_per_reading: i64,
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
                    vault(true),
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
                seconds_per_reading: 0,
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
            Ok(1_700_000_000
                + 5 * i64::from(self.pauses.get())
                + self.seconds_per_reading * i64::from(self.reads.get()))
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

    /// Each way staging can be down is its own MISSING line, once the deadline has passed.
    #[test]
    fn each_fault_is_named() -> Result<()> {
        let faults: [(Fault, &str); 9] = [
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
                |s| s.api = 200,
                "the API answers only through Access (200 without credentials)",
            ),
        ];
        for (fault, line) in faults {
            let mut staging = Staging::up();
            fault(&mut staging);
            let text = run(&staging)?;
            assert_eq!(missing(&text), [line], "{text}");
            assert_eq!(i64::from(staging.pauses.get()), SECONDS / 5, "{line}");
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
        staging.bindings.clear();
        staging.seconds_per_reading = 60;
        run(&staging)?;
        assert_eq!(
            staging.pauses.get(),
            7,
            "readings that take a minute each still stop at the deadline"
        );
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

    /// Until the tunnel connector lands, an unhealthy tunnel or an API Access cannot reach is a
    /// pending note that fails nothing; an API that answers without Access fails all the same; and
    /// with nothing pending, both are MISSING.
    #[test]
    fn the_edge_waits_on_the_connector_but_never_on_an_open_api() -> Result<()> {
        let mut staging = Staging::up();
        staging.tunnel = "down";
        staging.api = 502;
        let text = run(&staging)?;
        assert_eq!(missing(&text), Vec::<&str>::new(), "{text}");
        assert_eq!(staging.pauses.get(), 0);
        for line in [
            "[pending #1539] the tunnel is healthy (Cloudflare reports down)",
            "[pending #1539] the API answers only through Access (502 without credentials)",
        ] {
            assert!(text.contains(line), "{line}: {text}");
        }
        let list = checks(&|args| staging.kubectl(args), &staging, &edge(), None)?;
        assert_eq!(
            missing(&list.render_titled("infra verify (staging)")),
            [
                "the tunnel is healthy (Cloudflare reports down)",
                "the API answers only through Access (502 without credentials)",
            ]
        );
        assert!(!list.complete());
        let mut staging = Staging::up();
        staging.api = 200;
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            ["the API answers only through Access (200 without credentials)"],
            "{text}"
        );
        Ok(())
    }

    /// An unsealed vault holds; a sealed one, after the apply initialized it, fails (DEC-2052).
    #[test]
    fn a_sealed_vault_fails() -> Result<()> {
        let staging = Staging::up();
        let text = run(&staging)?;
        assert!(
            text.contains("[ok]      pod ws-ws1/vault-0 is Running and Ready, so unsealed"),
            "{text}"
        );
        let mut staging = Staging::up();
        staging.pods[5] = vault(false);
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            ["pod ws-ws1/vault-0 is Running and Ready, so unsealed"],
            "{text}"
        );
        Ok(())
    }
}
