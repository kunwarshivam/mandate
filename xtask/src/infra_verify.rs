//! Whether staging is up (E21-58, DEC-2050): the read-only check staging's apply runs after lane
//! GO's bootstrap, with the same short-lived kubeconfig and inside the same control-plane window
//! (DEC-2028), so it answers "is staging up?" without an agent touching the cluster. It reads
//! Flux's objects, the platform's and the workspaces' pods, the kill-switch admission bindings and
//! the signature webhook, whether Cilium runs the DNS proxy the FQDN egress policies need
//! (DEC-2058), the tunnel's health from Cloudflare, and the API's answer through
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

/// The change the edge's lines wait on, if any: while one is named, an unhealthy tunnel or an API
/// Access cannot reach is a pending note, not a failure (DEC-2050 item 3). The tunnel's connector
/// now runs beside the API (DEC-2056), so nothing is pending and both fail. An API that answers
/// without Access fails always.
const EDGE_PENDING: Option<&str> = None;

/// `kubectl` against the cluster, its arguments, answering what it printed: the check runs only
/// `get` and `logs`.
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
    let mut get = vec!["get"];
    get.extend(args);
    let items = kubectl(&get)
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
            let what = format!("{kind} {} is Ready", where_is(object));
            if is_ready(object) {
                list.check(true, what);
            } else {
                list.check(false, format!("{what} ({})", flux_why(object)));
            }
        }
    }
}

/// What a Flux object's condition messages say, as fixed class words alone: a message may quote a
/// value, an image or a path, so none is printed, only which of these it matched.
const FLUX_CLASSES: [(&str, &str); 16] = [
    ("dependency", "dependency"),
    ("context deadline exceeded", "timeout"),
    ("timed out", "timeout"),
    ("schema", "values-schema"),
    ("admission webhook", "admission-denied"),
    ("denied the request", "admission-denied"),
    ("signature", "signature"),
    ("verification failed", "signature"),
    ("hooks failed", "hook-failed"),
    ("failed pre-", "hook-failed"),
    ("failed post-", "hook-failed"),
    ("immutable", "immutable-field"),
    ("not found", "missing-object"),
    ("forbidden", "forbidden"),
    ("unauthorized", "auth-failed"),
    ("retries exhausted", "retries-exhausted"),
];

/// A HelmRelease's conditions that hold the last Helm action's own error, each with the status
/// that names a failure: `Released` is `False` when an install or upgrade failed, and `Remediated`
/// is `True` once helm-controller rolled back or uninstalled after one. While a release is retried,
/// Ready and Stalled say only that it is progressing or out of retries, so these carry the cause,
/// such as an admission webhook's denial.
const HELM_ACTION_CONDITIONS: [(&str, &str); 2] = [("Released", "False"), ("Remediated", "True")];

/// Why a Flux object is not Ready: its Ready condition's status and reason word, a Stalled
/// condition's reason, a failed Helm action's conditions' reasons ([`HELM_ACTION_CONDITIONS`]), and
/// the classes their messages match; a message that matches none is `unclassified`.
fn flux_why(object: &Value) -> String {
    let conditions = object["status"]["conditions"].as_array();
    let find =
        |kind: &str| conditions.and_then(|all| all.iter().find(|c| text(c, &["type"]) == kind));
    let Some(ready) = find("Ready") else {
        return "no Ready condition".to_owned();
    };
    let mut said = word(text(ready, &["status"]));
    let reason = word(text(ready, &["reason"]));
    if !reason.is_empty() {
        said.push_str(&format!(": {reason}"));
    }
    let mut messages = vec![text(ready, &["message"])];
    if let Some(stalled) = find("Stalled").filter(|c| text(c, &["status"]) == "True") {
        said.push_str(&format!("; Stalled: {}", word(text(stalled, &["reason"]))));
        messages.push(text(stalled, &["message"]));
    }
    for (kind, failing) in HELM_ACTION_CONDITIONS {
        if let Some(condition) = find(kind).filter(|c| text(c, &["status"]) == failing) {
            said.push_str(&format!("; {kind}: {}", word(text(condition, &["reason"]))));
            messages.push(text(condition, &["message"]));
        }
    }
    let lower = messages.join("\n").to_lowercase();
    let mut found: Vec<&str> = Vec::new();
    for (needle, class) in FLUX_CLASSES {
        if lower.contains(needle) && !found.contains(&class) {
            found.push(class);
        }
    }
    if found.is_empty() && !lower.trim().is_empty() {
        found.push("unclassified");
    }
    if !found.is_empty() {
        said.push_str(&format!("; message {}", found.join(", ")));
    }
    said
}

/// Every pod of the platform and the workspaces, but a finished Job's, Running with every
/// container ready, and at least one in each platform namespace.
fn pods(list: &mut Checklist, kubectl: Kubectl<'_>) {
    let Some(items) = listed(list, kubectl, "pods", &["pods", "-A", "-o", "json"]) else {
        return;
    };
    for pod in items.iter().filter(|pod| ours(pod)) {
        if text(pod, &["status", "phase"]) == "Succeeded" || !job_of(pod).is_empty() {
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
        if !up {
            let mut detail = why(pod);
            if let Some(container) = crashing(pod) {
                let logs = kubectl(&[
                    "logs",
                    text(pod, &["metadata", "name"]),
                    "--namespace",
                    text(pod, &["metadata", "namespace"]),
                    "--container",
                    container,
                    "--previous",
                    "--tail=30",
                ]);
                let classes = logs.map_or_else(|_| vec!["unreadable"], |text| log_classes(&text));
                if !classes.is_empty() {
                    detail.push_str(&format!("; logs {}", classes.join(", ")));
                }
            }
            what.push_str(&format!(" ({detail})"));
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
    jobs(list, kubectl, &items);
}

/// Whether an object is in a namespace the check reads: the platform's or a workspace's.
fn ours(object: &Value) -> bool {
    let namespace = text(object, &["metadata", "namespace"]);
    PLATFORM_NAMESPACES.contains(&namespace) || namespace.starts_with("ws-")
}

/// The Job that owns a pod, or `""`: a Job's pods are judged by the Job, since one that retried
/// leaves its failed pods behind.
fn job_of(pod: &Value) -> &str {
    pod["metadata"]["ownerReferences"]
        .as_array()
        .and_then(|owners| owners.iter().find(|o| text(o, &["kind"]) == "Job"))
        .map_or("", |owner| text(owner, &["name"]))
}

/// A Kubernetes status word as printed: letters and digits only, so nothing but the word reaches
/// the log.
fn word(raw: &str) -> String {
    raw.chars()
        .filter(char::is_ascii_alphanumeric)
        .take(40)
        .collect()
}

/// Why a pod is not up, from its status alone: its phase, its own reason, the first reason a
/// container is waiting or terminated with and how its last run ended, and, for a pod the
/// scheduler left unplaced, that condition's reason and [`SCHEDULING_CLASSES`]. No message or log
/// is printed, since either may hold anything.
fn why(pod: &Value) -> String {
    let status = &pod["status"];
    let mut words = vec![word(text(status, &["phase"]))];
    words.push(word(text(status, &["reason"])));
    let containers = ["initContainerStatuses", "containerStatuses"]
        .iter()
        .filter_map(|key| status[*key].as_array())
        .flatten();
    for container in containers {
        let reason = [
            &["state", "waiting", "reason"][..],
            &["state", "terminated", "reason"],
        ]
        .iter()
        .map(|path| text(container, path))
        .find(|reason| !reason.is_empty() && *reason != "Completed");
        if let Some(reason) = reason {
            words.push(word(reason));
            if let Some(code) = container["state"]["terminated"]["exitCode"].as_i64() {
                words.push(format!("exit {code}"));
            }
            let last = &container["lastState"]["terminated"];
            if !text(last, &["reason"]).is_empty() {
                words.push(format!("last {}", word(text(last, &["reason"]))));
                if let Some(code) = last["exitCode"].as_i64() {
                    words.push(format!("exit {code}"));
                }
            }
            break;
        }
    }
    let unscheduled = status["conditions"].as_array().and_then(|all| {
        all.iter()
            .find(|c| text(c, &["type"]) == "PodScheduled" && text(c, &["status"]) == "False")
    });
    if let Some(condition) = unscheduled {
        words.push(word(text(condition, &["reason"])));
        let lower = text(condition, &["message"]).to_lowercase();
        for (needle, class) in SCHEDULING_CLASSES {
            if lower.contains(needle) && !words.iter().any(|said| said == class) {
                words.push(class.to_owned());
            }
        }
    }
    let mut said: Vec<String> = words.into_iter().filter(|w| !w.is_empty()).collect();
    if said == ["Running"] {
        said.push("not ready".to_owned());
    }
    if said.is_empty() {
        "no status".to_owned()
    } else {
        said.join(": ")
    }
}

/// Why the scheduler left a pod unplaced, as fixed class words alone: its message names nodes and
/// counts, so only which of these it matched is printed.
const SCHEDULING_CLASSES: [(&str, &str); 9] = [
    ("insufficient cpu", "cpu"),
    ("insufficient memory", "memory"),
    ("unbound immediate persistentvolumeclaims", "unbound-pvc"),
    ("persistentvolumeclaim", "pvc"),
    ("volume node affinity conflict", "volume-zone"),
    ("node affinity", "affinity"),
    ("untolerated taint", "taint"),
    ("too many pods", "pod-limit"),
    ("max volume count", "volume-limit"),
];

/// The first container of `pod` waiting in `CrashLoopBackOff`, whose last run's log says why it
/// ended.
fn crashing(pod: &Value) -> Option<&str> {
    pod["status"]["containerStatuses"]
        .as_array()?
        .iter()
        .find(|c| text(c, &["state", "waiting", "reason"]) == "CrashLoopBackOff")
        .map(|c| text(c, &["name"]))
        .filter(|name| !name.is_empty())
}

/// What a failing Job's last log lines say, as fixed class words alone: its lines may echo a
/// connection string or SQL, so none of them is printed, only which of these it matched.
const LOG_CLASSES: [(&str, &str); 33] = [
    ("password authentication failed", "auth-failed"),
    ("connection refused", "connection-refused"),
    ("timeout expired", "timeout"),
    ("timed out", "timeout"),
    ("could not translate host name", "dns"),
    ("name or service not known", "dns"),
    ("does not exist", "missing-object"),
    ("permission denied", "permission-denied"),
    ("ssl", "tls"),
    ("a migration is never edited", "migration-edited"),
    ("workspace_not_new", "workspace_not_new"),
    ("members_exist", "members_exist"),
    ("workspace_not_open", "workspace_not_open"),
    ("vault", "vault"),
    ("approle", "approle"),
    ("subject-links", "subject-links"),
    ("subject link", "subject-links"),
    ("founding", "founding"),
    ("issuer", "issuer"),
    ("no such file", "missing-file"),
    ("address already in use", "port-in-use"),
    ("panicked", "panic"),
    ("release_unsigned", "release-unsigned"),
    ("release_signature", "release-signature"),
    ("release_unreadable", "release-unreadable"),
    ("required setting is absent or malformed", "setting"),
    ("session signing key", "session-key"),
    ("secret-named variable", "secret-in-environment"),
    ("database url carries a password", "password-in-url"),
    ("connection settings are refused", "journal-settings"),
    (
        "control stream is not shown to be paper",
        "control-stream-not-paper",
    ),
    ("bind address", "bind"),
    ("missing or invalid", "binding-incomplete"),
];

/// The classes `logs` matches, in [`LOG_CLASSES`]' order, each once; a line that says it failed
/// but matches none is `unclassified`.
fn log_classes(logs: &str) -> Vec<&'static str> {
    let lower = logs.to_lowercase();
    let mut found: Vec<&'static str> = Vec::new();
    for (needle, class) in LOG_CLASSES {
        if lower.contains(needle) && !found.contains(&class) {
            found.push(class);
        }
    }
    let failed = lower
        .lines()
        .any(|line| line.contains("error") || line.contains("fatal") || line.contains("refused"));
    if found.is_empty() && failed {
        found.push("unclassified");
    }
    found
}

/// Every Job of the platform's and the workspaces' is Complete; one that is not says whether it
/// failed, how many pods it has running and failed, and why its pods are not up.
fn jobs(list: &mut Checklist, kubectl: Kubectl<'_>, pods: &[Value]) {
    let Some(items) = listed(list, kubectl, "jobs", &["jobs", "-A", "-o", "json"]) else {
        return;
    };
    for job in items.iter().filter(|job| ours(job)) {
        let condition = |kind: &str| {
            job["status"]["conditions"].as_array().and_then(|all| {
                all.iter()
                    .find(|c| text(c, &["type"]) == kind && text(c, &["status"]) == "True")
            })
        };
        let what = format!("job {} is Complete", where_is(job));
        if condition("Complete").is_some() {
            list.check(true, what);
            continue;
        }
        let mut detail = format!(
            "active {}, failed {}",
            job["status"]["active"].as_u64().unwrap_or_default(),
            job["status"]["failed"].as_u64().unwrap_or_default()
        );
        for held in job["status"]["conditions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| text(c, &["status"]) == "True")
        {
            detail.push_str(&format!(
                "; {}: {}",
                word(text(held, &["type"])),
                word(text(held, &["reason"]))
            ));
        }
        let mut seen: Vec<String> = Vec::new();
        for pod in pods.iter().filter(|pod| {
            text(pod, &["metadata", "namespace"]) == text(job, &["metadata", "namespace"])
                && job_of(pod) == text(job, &["metadata", "name"])
                && text(pod, &["status", "phase"]) != "Succeeded"
        }) {
            let reason = why(pod);
            if !seen.contains(&reason) {
                seen.push(reason);
            }
        }
        if !seen.is_empty() {
            detail.push_str(&format!("; pods {}", seen.join(", ")));
        }
        let name = format!("job/{}", text(job, &["metadata", "name"]));
        let logs = kubectl(&[
            "logs",
            &name,
            "--namespace",
            text(job, &["metadata", "namespace"]),
            "--tail=20",
            "--all-containers",
        ]);
        let classes = logs.map_or_else(|_| vec!["unreadable"], |text| log_classes(&text));
        if !classes.is_empty() {
            detail.push_str(&format!("; logs {}", classes.join(", ")));
        }
        list.check(false, format!("{what} ({detail})"));
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

/// Whether Cilium runs the L7 DNS proxy that staging's FQDN egress policies need (DEC-2058): its
/// config must not turn `enable-l7-proxy` off. An unset key is Cilium's default, on; a cluster with
/// no Cilium config enforces no FQDN policy, so it fails.
fn cilium(list: &mut Checklist, kubectl: Kubectl<'_>) {
    let Some(items) = listed(
        list,
        kubectl,
        "Cilium's config",
        &[
            "configmaps",
            "-n",
            "kube-system",
            "--field-selector",
            "metadata.name=cilium-config",
            "-o",
            "json",
        ],
    ) else {
        return;
    };
    let proxied = items
        .iter()
        .any(|config| config["data"]["enable-l7-proxy"].as_str() != Some("false"));
    list.check(
        proxied,
        "Cilium runs the L7 DNS proxy the FQDN egress policies need",
    );
}

/// What cloudflared's last log lines are about, as fixed class words alone: a line may quote the
/// tunnel's token, an address or a hostname, so none is printed.
const CLOUDFLARED_CLASSES: [(&str, &str); 10] = [
    ("registered tunnel connection", "registered"),
    ("unauthorized", "auth-failed"),
    ("invalid tunnel secret", "auth-failed"),
    ("invalid token", "auth-failed"),
    ("connection refused", "connection-refused"),
    ("timeout", "timeout"),
    ("deadline exceeded", "timeout"),
    ("no such host", "dns"),
    ("server misbehaving", "dns"),
    ("i/o timeout", "timeout"),
];

/// Which workspace pod runs cloudflared, and what its last lines are about: read only once
/// Cloudflare reports the tunnel down, so a healthy staging reads no log.
fn cloudflared_why(kubectl: Kubectl<'_>) -> String {
    let pods = kubectl(&["get", "pods", "-A", "-o", "json"])
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|listing| listing["items"].as_array().cloned())
        .unwrap_or_default();
    let Some(pod) = pods.iter().find(|pod| {
        text(pod, &["metadata", "namespace"]).starts_with("ws-")
            && pod["spec"]["containers"]
                .as_array()
                .is_some_and(|all| all.iter().any(|c| text(c, &["name"]) == "cloudflared"))
    }) else {
        return "no cloudflared pod".to_owned();
    };
    let at = where_is(pod);
    let logs = kubectl(&[
        "logs",
        text(pod, &["metadata", "name"]),
        "--namespace",
        text(pod, &["metadata", "namespace"]),
        "--container",
        "cloudflared",
        "--tail=50",
    ]);
    let Ok(logs) = logs else {
        return format!("cloudflared {at} logs unreadable");
    };
    let lower = logs.to_lowercase();
    let mut found: Vec<&str> = Vec::new();
    for (needle, class) in CLOUDFLARED_CLASSES {
        if lower.contains(needle) && !found.contains(&class) {
            found.push(class);
        }
    }
    let failed = lower.lines().any(|line| line.contains("err"));
    if failed && found.iter().all(|class| *class == "registered") {
        found.push("unclassified");
    }
    if found.is_empty() {
        found.push("quiet");
    }
    format!("cloudflared {at} logs {}", found.join(", "))
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
    kubectl: Kubectl<'_>,
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
                format!(
                    "the tunnel is healthy (Cloudflare reports {said}; {})",
                    cloudflared_why(kubectl)
                ),
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
    cilium(&mut list, kubectl);
    edge_checks(&mut list, kubectl, http, edge, pending);
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

    fn in_job(mut found: Value, name: &str) -> Value {
        found["metadata"]["ownerReferences"] = json!([{"kind": "Job", "name": name}]);
        found
    }

    fn job(name: &str, condition: &str) -> Value {
        json!({
            "metadata": {"name": name, "namespace": "ws-ws1"},
            "status": {
                "active": 1,
                "failed": 2,
                "conditions": [{"type": condition, "status": "True", "reason": "BackoffLimitExceeded"}],
            },
        })
    }

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
        jobs: Vec<Value>,
        cilium: Vec<Value>,
        logs: &'static str,
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
                    in_job(pod("ws-ws1", "migrate-x", "Succeeded", &[false]), "migrate"),
                    vault(true),
                    pod("kube-system", "coredns-x", "Pending", &[false]),
                ],
                bindings: BINDINGS
                    .iter()
                    .map(|name| {
                        json!({"metadata": {"name": name}, "spec": {"policyName": name, "validationActions": ["Deny"]}})
                    })
                    .collect(),
                jobs: vec![job("migrate", "Complete")],
                cilium: vec![cilium_config(Some("true"))],
                logs: "",
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
            if args.first() == Some(&"logs") {
                return Ok(self.logs.to_owned());
            }
            assert_eq!(
                args.first(),
                Some(&"get"),
                "verify only runs get and logs: {args:?}"
            );
            let args = &args[1..];
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
                "jobs" => self.jobs.clone(),
                "configmaps" => self.cilium.clone(),
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

    fn cilium_config(l7_proxy: Option<&str>) -> Value {
        let mut config =
            json!({"metadata": {"name": "cilium-config", "namespace": "kube-system"}, "data": {}});
        if let Some(value) = l7_proxy {
            config["data"]["enable-l7-proxy"] = json!(value);
        }
        config
    }

    /// The FQDN egress policies hold only while Cilium runs its DNS proxy (DEC-2058): a cluster
    /// whose config turns it off, or has no Cilium config, fails; Cilium's default, an unset key,
    /// holds.
    #[test]
    fn fqdn_egress_needs_ciliums_dns_proxy() -> Result<()> {
        let line = "Cilium runs the L7 DNS proxy the FQDN egress policies need";
        let mut staging = Staging::up();
        let text = run(&staging)?;
        assert!(
            text.lines()
                .any(|row| row.trim_start().starts_with("[ok]") && row.ends_with(line)),
            "{text}"
        );
        staging.cilium = vec![cilium_config(None)];
        assert_eq!(missing(&run(&staging)?), Vec::<&str>::new());
        for config in [vec![cilium_config(Some("false"))], Vec::new()] {
            staging.cilium = config;
            let text = run(&staging)?;
            assert_eq!(missing(&text), [line], "{text}");
        }
        Ok(())
    }

    fn run(staging: &Staging) -> Result<String> {
        let list = verify(&|args| staging.kubectl(args), staging, &edge())?;
        Ok(list.render_titled("infra verify (staging)"))
    }

    fn failing(kind: &str, name: &str, conditions: Value) -> Value {
        json!({
            "kind": kind,
            "metadata": {"name": name, "namespace": "flux-system"},
            "status": {"conditions": conditions},
        })
    }

    /// A Flux object that is not Ready says why in its condition's reason word and the fixed
    /// classes its message matches, never the message itself, which may quote values or images.
    #[test]
    fn a_flux_object_that_is_not_ready_says_why_without_its_message() -> Result<()> {
        let cases = [
            (
                json!([{"type": "Ready", "status": "False", "reason": "DependencyNotReady",
                    "message": "dependency 'flux-system/journal' is not ready"}]),
                "(False: DependencyNotReady; message dependency)",
            ),
            (
                json!([
                    {"type": "Ready", "status": "False", "reason": "UpgradeFailed",
                        "message": "Helm upgrade failed for release ws-ws1/workspace: context deadline exceeded"},
                    {"type": "Stalled", "status": "True", "reason": "RetriesExceeded",
                        "message": "upgrade retries exhausted"},
                ]),
                "(False: UpgradeFailed; Stalled: RetriesExceeded; message timeout, retries-exhausted)",
            ),
            (
                json!([{"type": "Ready", "status": "False", "reason": "InstallFailed",
                    "message": "admission webhook \"policy.sigstore.dev\" denied the request: validation failed: no matching signatures: ghcr.io/kunwarshivam/mandate/secret-token-xyz"}]),
                "(False: InstallFailed; message admission-denied, signature)",
            ),
            (
                json!([{"type": "Ready", "status": "False", "reason": "InstallFailed",
                    "message": "values don't meet the specifications of the schema(s): api.org: Does not match pattern"}]),
                "(False: InstallFailed; message values-schema)",
            ),
            (
                json!([{"type": "Ready", "status": "False", "reason": "Progressing",
                    "message": "something new and odd: hunter2"}]),
                "(False: Progressing; message unclassified)",
            ),
            (
                json!([
                    {"type": "Ready", "status": "Unknown", "reason": "Progressing",
                        "message": "Running 'upgrade' action with timeout of 5m0s"},
                    {"type": "Stalled", "status": "True", "reason": "RetriesExceeded",
                        "message": "Failed to upgrade after 1 attempt(s)"},
                    {"type": "Released", "status": "False", "reason": "UpgradeFailed",
                        "message": "Helm upgrade failed for release ws-ws1/workspace: admission webhook \"policy.sigstore.dev\" denied the request: validation failed: no signatures found: ghcr.io/kunwarshivam/mandate/secret-token-xyz"},
                    {"type": "Remediated", "status": "True", "reason": "RollbackSucceeded",
                        "message": "Helm rollback to previous release ws-ws1/workspace.v1 succeeded: hunter2"},
                ]),
                "(Unknown: Progressing; Stalled: RetriesExceeded; Released: UpgradeFailed; Remediated: RollbackSucceeded; message admission-denied, signature)",
            ),
            (
                json!([
                    {"type": "Ready", "status": "True", "reason": "UpgradeSucceeded", "message": "ok"},
                    {"type": "Released", "status": "True", "reason": "UpgradeSucceeded",
                        "message": "Helm upgrade succeeded: hunter2"},
                ]),
                "",
            ),
            (json!([]), "(no Ready condition)"),
        ];
        for (conditions, why) in cases {
            let mut staging = Staging::up();
            staging.flux[3] = failing("HelmRelease", "workspace", conditions);
            let text = run(&staging)?;
            if why.is_empty() {
                assert!(missing(&text).is_empty(), "{text}");
            } else {
                assert_eq!(
                    missing(&text),
                    [format!("HelmRelease flux-system/workspace is Ready {why}").as_str()],
                    "{text}"
                );
            }
            for secret in [
                "secret-token-xyz",
                "hunter2",
                "api.org",
                "'flux-system/journal'",
            ] {
                assert!(!text.contains(secret), "{secret} leaked: {text}");
            }
        }
        Ok(())
    }

    fn tunnel_pod(namespace: &str, name: &str) -> Value {
        let mut found = pod(namespace, name, "Running", &[true]);
        found["spec"] = json!({"containers": [{"name": "cloudflared"}]});
        found
    }

    /// A tunnel Cloudflare reports down says what its cloudflared pod's last lines are about, as
    /// fixed classes, never the lines themselves, which may quote the token or a hostname.
    #[test]
    fn a_down_tunnel_says_what_cloudflared_logs_without_them() -> Result<()> {
        let cases = [
            (
                "2026-10-19T15:00:00Z ERR Register tunnel error from server side error=\"Unauthorized: Invalid tunnel secret eyJhIjoiS0VZIn0\"",
                "cloudflared ws-ws1/cloudflared-x logs auth-failed",
            ),
            (
                "ERR Failed to dial a quic connection error=\"timeout: no recent network activity\" connIndex=0 ip=198.41.192.7",
                "cloudflared ws-ws1/cloudflared-x logs timeout",
            ),
            (
                "ERR edge discovery: error looking up Cloudflare edge IPs: lookup _v2-origintunneld._tcp.argotunnel.com: no such host",
                "cloudflared ws-ws1/cloudflared-x logs dns",
            ),
            (
                "INF Registered tunnel connection connIndex=0\nERR  error=\"Unable to reach the origin service. dial tcp 127.0.0.1:8080: connect: connection refused\"",
                "cloudflared ws-ws1/cloudflared-x logs registered, connection-refused",
            ),
            (
                "ERR something odd happened secret-xyz",
                "cloudflared ws-ws1/cloudflared-x logs unclassified",
            ),
        ];
        for (logs, why) in cases {
            let mut staging = Staging::up();
            staging.tunnel = "down";
            staging.pods.push(tunnel_pod("ws-ws1", "cloudflared-x"));
            staging.logs = logs;
            let text = run(&staging)?;
            assert_eq!(
                missing(&text),
                [format!("the tunnel is healthy (Cloudflare reports down; {why})").as_str()],
                "{text}"
            );
            for secret in [
                "eyJhIjoiS0VZIn0",
                "198.41.192.7",
                "argotunnel",
                "secret-xyz",
                "127.0.0.1",
            ] {
                assert!(!text.contains(secret), "{secret} leaked: {text}");
            }
        }
        let mut staging = Staging::up();
        staging.tunnel = "down";
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            ["the tunnel is healthy (Cloudflare reports down; no cloudflared pod)"],
            "{text}"
        );
        Ok(())
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
                "HelmRelease flux-system/workspace is Ready (False)",
            ),
            (
                |s| {
                    s.flux.remove(1);
                },
                "Flux has an OCIRepository",
            ),
            (
                |s| s.pods[3] = pod("ws-ws1", "journal-1", "Running", &[true, false]),
                "pod ws-ws1/journal-1 is Running and Ready (Running: not ready)",
            ),
            (
                |s| s.pods[1] = pod("cnpg-system", "cnpg-x", "Failed", &[false]),
                "pod cnpg-system/cnpg-x is Running and Ready (Failed)",
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

    /// The tunnel's connector runs beside the API (DEC-2056), so an unhealthy tunnel or an API
    /// Access cannot reach fails; a pending change, when one is named, turns either into a note
    /// that fails nothing; and an API that answers without Access fails whatever is pending.
    #[test]
    fn the_edge_fails_unless_a_change_is_pending_but_never_on_an_open_api() -> Result<()> {
        let mut staging = Staging::up();
        staging.tunnel = "down";
        staging.api = 502;
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            [
                "the tunnel is healthy (Cloudflare reports down; no cloudflared pod)",
                "the API answers only through Access (502 without credentials)",
            ],
            "{text}"
        );
        let list = checks(&|args| staging.kubectl(args), &staging, &edge(), Some("#1"))?;
        let text = list.render_titled("infra verify (staging)");
        assert_eq!(missing(&text), Vec::<&str>::new(), "{text}");
        for line in [
            "[pending #1] the tunnel is healthy (Cloudflare reports down; no cloudflared pod)",
            "[pending #1] the API answers only through Access (502 without credentials)",
        ] {
            assert!(text.contains(line), "{line}: {text}");
        }
        let mut staging = Staging::up();
        staging.api = 200;
        let list = checks(&|args| staging.kubectl(args), &staging, &edge(), Some("#1"))?;
        assert_eq!(
            missing(&list.render_titled("infra verify (staging)")),
            ["the API answers only through Access (200 without credentials)"]
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
            ["pod ws-ws1/vault-0 is Running and Ready, so unsealed (Running: not ready)"],
            "{text}"
        );
        Ok(())
    }

    /// A Job's pods are judged by the Job: a Job that retried and completed holds, its failed pods
    /// aside; one that is still retrying or failed is MISSING with why its pods are not up. A pod
    /// that is not up says its phase and container reason, and nothing else (DEC-2050).
    #[test]
    fn jobs_are_judged_by_completion_and_failures_say_why() -> Result<()> {
        let mut staging = Staging::up();
        let mut pulling = pod("ws-ws1", "migrate-b", "Pending", &[false]);
        pulling["status"]["containerStatuses"][0]["state"] =
            json!({"waiting": {"reason": "ImagePullBackOff", "message": "secret-ish detail"}});
        let mut failed = pod("ws-ws1", "migrate-a", "Failed", &[false]);
        failed["status"]["containerStatuses"][0]["state"] =
            json!({"terminated": {"reason": "Error", "exitCode": 1}});
        staging.pods.push(in_job(failed.clone(), "migrate"));
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            Vec::<&str>::new(),
            "a completed Job holds: {text}"
        );
        assert!(
            text.contains("[ok]      job ws-ws1/migrate is Complete"),
            "{text}"
        );

        staging.jobs = vec![
            json!({"metadata": {"name": "migrate", "namespace": "ws-ws1"}, "status": {"active": 1, "failed": 1}}),
        ];
        staging.pods.push(in_job(pulling.clone(), "migrate"));
        staging.logs =
            "psql: error: connection to server at \"journal-rw\" failed: timeout expired";
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            [
                "job ws-ws1/migrate is Complete (active 1, failed 1; pods Failed: Error: exit 1, Pending: ImagePullBackOff; logs timeout)"
            ],
            "{text}"
        );
        staging.jobs = vec![job("migrate", "Failed")];
        staging.logs = "psql: error: connection to server failed: FATAL:  password authentication \
                        failed for user \"mandate_migrator\" postgresql://u:hunter2@db";
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            [
                "job ws-ws1/migrate is Complete (active 1, failed 2; Failed: BackoffLimitExceeded; pods Failed: Error: exit 1, Pending: ImagePullBackOff; logs auth-failed)"
            ],
            "{text}"
        );
        for leak in ["secret-ish", "hunter2", "mandate_migrator", "postgresql"] {
            assert!(!text.contains(leak), "{leak}: {text}");
        }
        assert_eq!(
            log_classes("applied: 0001.sql\nERROR: relation x\n"),
            ["unclassified"]
        );
        assert!(log_classes("already applied: 0001.sql\n").is_empty());
        assert_eq!(
            log_classes(
                "refused: MANDATE_API_INTERNAL_PAPER_WORKSPACE is on, and the control stream is not shown to be paper\n"
            ),
            ["control-stream-not-paper"]
        );
        assert_eq!(
            log_classes("refused: a required setting is absent or malformed: MANDATE_API_X\n"),
            ["setting"]
        );
        assert_eq!(
            log_classes("refused: finish off: MANDATE_API_DATABASE_URL missing or invalid\n"),
            ["binding-incomplete"]
        );
        assert_eq!(log_classes("release_unsigned\n"), ["release-unsigned"]);
        assert_eq!(log_classes("refused: something new\n"), ["unclassified"]);

        let mut staging = Staging::up();
        staging.pods[3] = pulling;
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            ["pod ws-ws1/migrate-b is Running and Ready (Pending: ImagePullBackOff)"],
            "{text}"
        );
        Ok(())
    }

    #[test]
    fn a_crashing_pod_says_how_its_last_run_ended_and_an_unplaced_one_why() -> Result<()> {
        let mut staging = Staging::up();
        staging.pods[3] = json!({
            "metadata": {"name": "api-x", "namespace": "ws-ws1"},
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "name": "api",
                    "ready": false,
                    "state": {"waiting": {"reason": "CrashLoopBackOff"}},
                    "lastState": {"terminated": {"reason": "Error", "exitCode": 1}},
                }],
            },
        });
        staging.pods.push(json!({
            "metadata": {"name": "data-x", "namespace": "ws-ws1"},
            "status": {
                "phase": "Pending",
                "conditions": [{
                    "type": "PodScheduled",
                    "status": "False",
                    "reason": "Unschedulable",
                    "message": "0/2 nodes are available: pod has unbound immediate PersistentVolumeClaims node-7f3",
                }],
            },
        }));
        staging.logs = "api: reading the vault AppRole secret id at /vault/secret-id: token s.hunter2 rejected";
        let text = run(&staging)?;
        assert_eq!(
            missing(&text),
            [
                "pod ws-ws1/api-x is Running and Ready (Running: CrashLoopBackOff: last Error: exit 1; logs vault, approle)",
                "pod ws-ws1/data-x is Running and Ready (Pending: Unschedulable: unbound-pvc: pvc)",
            ],
            "{text}"
        );
        for leak in ["hunter2", "/vault/secret-id", "node-7f3"] {
            assert!(!text.contains(leak), "{leak}: {text}");
        }
        Ok(())
    }
}
