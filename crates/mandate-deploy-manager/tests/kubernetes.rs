//! The Kubernetes adapter (DEC-1170, D-3) against a fake API server on loopback: what it sends to
//! list, create and delete pods, how it reads a pod list into instances, and that a pod it cannot
//! read hides nothing. In the cell it speaks HTTPS with the cluster's CA only; here the client is
//! plain HTTP on loopback, which `in_cluster` refuses.

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::block_on;
use mandate_canon::Digest;
use mandate_connections::record::ConnectionId;
use mandate_deploy_manager::kubernetes::{InCluster, Kubernetes, MAX_BODY, SetupError};
use mandate_deploy_manager::{Instance, Launch, Orchestrator, Phase, Process, Slot, Unavailable};
use mandate_time::UtcNanos;
use mandate_vault::kubernetes::service_account_name;
use mandate_vault::policy::Identity;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    authorization: String,
    body: String,
}

/// Serves each scripted `(status, body)` to one request in turn, and records the requests.
fn server(script: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for (status, body) in script {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let mut length = 0;
            let mut authorization = String::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap();
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => length = value.trim().parse().unwrap(),
                    "authorization" => value.trim().clone_into(&mut authorization),
                    _ => {}
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            log.lock().unwrap().push(Seen {
                method,
                path,
                authorization,
                body: String::from_utf8(bytes).unwrap(),
            });
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (address, seen)
}

fn template() -> Value {
    json!({
        "metadata": {"labels": {"app.kubernetes.io/name": "runtime", "mandate.dev/workspace": "ws1"}},
        "spec": {
            "restartPolicy": "Always",
            "containers": [{"name": "runtime", "image": "runtime@sha256:00", "env": [{"name": "X", "value": "1"}]}]
        }
    })
}

fn exchange_template() -> Value {
    json!({
        "metadata": {"labels": {"app.kubernetes.io/name": "token-exchange", "mandate.dev/workspace": "ws1"}},
        "spec": {
            "restartPolicy": "Never",
            "containers": [{"name": "token-exchange", "image": "exchange@sha256:00", "command": ["/usr/local/bin/mandate-token-exchange"]}]
        }
    })
}

/// The request expiry [`launch`] gives a start with a lifetime: 2026-10-17T15:10:10Z and 5 ns.
fn expiry() -> UtcNanos {
    UtcNanos::from_parts(1_792_249_810, 5).unwrap()
}

/// A start's [`Launch`] with `lifetime`, [`expiry`] beside a lifetime, and no connection.
fn launch(lifetime: Option<Duration>) -> Launch {
    Launch {
        lifetime,
        expires_at: lifetime.and(Some(expiry())),
        connection: None,
    }
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn adapter(api: String) -> Kubernetes {
    let templates = BTreeMap::from([
        (Process::Runtime, template()),
        (Process::Executor, template()),
        (Process::TokenExchange, exchange_template()),
    ]);
    Kubernetes::with_client(
        client(),
        api,
        "cell".to_owned(),
        "token-xyz".to_owned(),
        "ws_01J8Z2".to_owned(),
        templates,
    )
    .unwrap()
}

fn pod(
    process: &str,
    subject: &str,
    generation: &str,
    phase: &str,
    ready: bool,
    deleting: bool,
) -> Value {
    let mut metadata = json!({
        "labels": {"mandate.dev/process": process, "mandate.dev/generation": generation},
        "annotations": {"mandate.dev/subject": subject}
    });
    if deleting {
        metadata["deletionTimestamp"] = json!("2026-10-17T15:00:00Z");
    }
    json!({
        "metadata": metadata,
        "status": {"phase": phase, "conditions": [{"type": "Ready", "status": if ready { "True" } else { "False" }}]}
    })
}

#[test]
fn a_pod_list_reads_as_instances_and_a_pod_being_deleted_may_still_run() {
    let list = json!({"items": [
        pod("runtime", "agent_a", "3", "Running", true, false),
        pod("runtime", "agent_b", "1", "Running", false, false),
        pod("executor", "ACCT_1", "2", "Pending", false, false),
        pod("executor", "acct_2", "1", "Failed", false, false),
        pod("runtime", "agent_c", "4", "Running", true, true),
        pod("runtime", "agent_d", "1", "Unknown", false, false),
        pod("executor", "acct_4", "1", "Pending", false, true),
    ]});
    let (api, seen) = server(vec![(200, list.to_string())]);
    let mut k = adapter(api);
    let got = block_on(k.observe()).unwrap();
    let want = [
        (Slot::runtime("agent_a"), 3, Phase::Running { ready: true }),
        (Slot::runtime("agent_b"), 1, Phase::Running { ready: false }),
        (Slot::executor("ACCT_1"), 2, Phase::Pending),
        (Slot::executor("acct_2"), 1, Phase::Exited),
        (Slot::runtime("agent_c"), 4, Phase::Unknown),
        (Slot::runtime("agent_d"), 1, Phase::Unknown),
        (Slot::executor("acct_4"), 1, Phase::Unknown),
    ];
    let want: Vec<Instance> = want
        .into_iter()
        .map(|(slot, generation, phase)| Instance {
            slot,
            generation,
            phase,
        })
        .collect();
    assert_eq!(got, want);
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].method, "GET");
    assert!(
        seen[0]
            .path
            .starts_with("/api/v1/namespaces/cell/pods?labelSelector="),
        "{}",
        seen[0].path
    );
    assert!(
        seen[0]
            .path
            .contains("app.kubernetes.io%2Fmanaged-by%3Dmandate-deploy-manager")
    );
    assert!(seen[0].path.contains("mandate.dev%2Fworkspace-key%3D"));
    assert_eq!(seen[0].authorization, "Bearer token-xyz");
}

#[test]
fn a_pod_it_cannot_read_makes_the_whole_list_unavailable() {
    for broken in [
        json!({"items": [{"metadata": {"labels": {"mandate.dev/process": "runtime"}, "annotations": {"mandate.dev/subject": "a"}}}]}),
        json!({"items": [{"metadata": {"labels": {"mandate.dev/process": "other", "mandate.dev/generation": "1"}, "annotations": {"mandate.dev/subject": "a"}}}]}),
        json!({"items": [{"metadata": {"labels": {"mandate.dev/process": "runtime", "mandate.dev/generation": "x"}, "annotations": {"mandate.dev/subject": "a"}}}]}),
        json!({"items": [{"metadata": {"labels": {"mandate.dev/process": "runtime", "mandate.dev/generation": "1"}}}]}),
        json!({"items": [{"status": {}}]}),
        json!({"kind": "PodList"}),
    ] {
        let (api, _) = server(vec![(200, broken.to_string())]);
        assert_eq!(
            block_on(adapter(api).observe()),
            Err(Unavailable),
            "{broken}"
        );
    }
    let (api, _) = server(vec![(200, "not json".to_owned())]);
    assert_eq!(block_on(adapter(api).observe()), Err(Unavailable));
    let (api, _) = server(vec![(403, "{}".to_owned())]);
    assert_eq!(block_on(adapter(api).observe()), Err(Unavailable));
    assert_eq!(
        block_on(adapter("http://127.0.0.1:1".to_owned()).observe()),
        Err(Unavailable)
    );
}

#[test]
fn a_start_creates_a_bare_pod_the_orchestrator_never_restarts() {
    let (api, seen) = server(vec![
        (201, "{}".to_owned()),
        (409, "{}".to_owned()),
        (500, "{}".to_owned()),
    ]);
    let mut k = adapter(api);
    let slot = Slot::runtime("Agent_A");
    assert_eq!(block_on(k.start(&slot, 7, &launch(None))), Ok(()));
    assert_eq!(
        block_on(k.start(&slot, 7, &launch(None))),
        Ok(()),
        "already created is done"
    );
    assert_eq!(block_on(k.start(&slot, 7, &launch(None))), Err(Unavailable));
    let seen = seen.lock().unwrap();
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/api/v1/namespaces/cell/pods")
    );
    let sent: Value = serde_json::from_str(&seen[0].body).unwrap();
    let name = sent["metadata"]["name"].as_str().unwrap();
    assert_eq!(name, k.pod_name(&slot, 7));
    assert!(
        name.starts_with("mandate-rt-") && name.ends_with("-g7"),
        "{name}"
    );
    assert!(
        name.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "{name}"
    );
    assert_eq!(sent["kind"], "Pod");
    assert_eq!(sent["apiVersion"], "v1");
    assert_eq!(sent["spec"]["restartPolicy"], "Never");
    let labels = &sent["metadata"]["labels"];
    assert_eq!(
        labels["app.kubernetes.io/managed-by"],
        "mandate-deploy-manager"
    );
    assert_eq!(
        labels["app.kubernetes.io/name"], "runtime",
        "the chart's labels are kept"
    );
    assert_eq!(labels["mandate.dev/process"], "runtime");
    assert_eq!(labels["mandate.dev/generation"], "7");
    let key = |text: &str| Digest::of(text.as_bytes()).to_hex()[..16].to_owned();
    assert_eq!(
        labels["mandate.dev/workspace-key"],
        key("ws_01J8Z2").as_str()
    );
    assert_eq!(
        labels["mandate.dev/workspace"], "ws1",
        "the chart's workspace label, which its network policies select, is kept"
    );
    assert_eq!(
        labels["mandate.dev/subject-key"],
        key("Agent_A").as_str(),
        "a pod is selectable by its subject's key"
    );
    assert_eq!(
        sent["metadata"]["annotations"]["mandate.dev/subject"],
        "Agent_A"
    );
    let env = sent["spec"]["containers"][0]["env"].as_array().unwrap();
    let names: Vec<&str> = env.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "X",
            "MANDATE_WORKSPACE",
            "MANDATE_PROCESS",
            "MANDATE_SUBJECT",
            "MANDATE_GENERATION"
        ]
    );
    let values: Vec<&str> = env.iter().map(|e| e["value"].as_str().unwrap()).collect();
    assert_eq!(
        values,
        ["1", "ws_01J8Z2", "runtime", "Agent_A", "7"],
        "a runtime names no account: it runs as the default one"
    );
}

#[test]
fn a_container_without_an_environment_gets_one() {
    let mut bare = template();
    bare["spec"]["containers"][0]
        .as_object_mut()
        .unwrap()
        .remove("env");
    let k = Kubernetes::with_client(
        client(),
        "http://127.0.0.1:1/".to_owned(),
        "cell".to_owned(),
        "t".to_owned(),
        "ws_01J8Z2".to_owned(),
        BTreeMap::from([(Process::Runtime, bare)]),
    )
    .unwrap();
    let pod = k
        .manifest(&Slot::runtime("agent_a"), 1, &launch(None))
        .unwrap();
    assert_eq!(
        pod["spec"]["containers"][0]["env"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(
        pod["metadata"]["name"]
            .as_str()
            .unwrap()
            .starts_with("mandate-rt-")
    );
    assert_eq!(
        k.manifest(&Slot::executor("acct_1"), 1, &launch(None)),
        None,
        "no template, no pod"
    );
}

#[test]
fn pod_names_differ_by_workspace_subject_and_generation() {
    let k = adapter("http://127.0.0.1:1".to_owned());
    let names = [
        k.pod_name(&Slot::runtime("agent_a"), 1),
        k.pod_name(&Slot::runtime("agent_a"), 2),
        k.pod_name(&Slot::runtime("agent_b"), 1),
        k.pod_name(&Slot::executor("agent_a"), 1),
        k.pod_name(&Slot::token_exchange("agent_a"), 1),
    ];
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn a_stop_deletes_the_generations_pod_and_a_pod_already_gone_is_stopped() {
    let (api, seen) = server(vec![
        (200, "{}".to_owned()),
        (404, "{}".to_owned()),
        (503, "{}".to_owned()),
    ]);
    let mut k = adapter(api);
    let slot = Slot::executor("acct_1");
    assert_eq!(block_on(k.stop(&slot, 2, None)), Ok(()));
    assert_eq!(block_on(k.stop(&slot, 2, None)), Ok(()));
    assert_eq!(block_on(k.stop(&slot, 2, None)), Err(Unavailable));
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].method, "DELETE");
    assert_eq!(
        seen[0].path,
        format!("/api/v1/namespaces/cell/pods/{}", k.pod_name(&slot, 2))
    );
}

#[test]
fn a_template_without_containers_is_refused() {
    for bad in [
        json!({}),
        json!({"spec": {}}),
        json!({"spec": {"containers": [1]}}),
    ] {
        let made = Kubernetes::with_client(
            client(),
            "http://127.0.0.1:1".to_owned(),
            "cell".to_owned(),
            "t".to_owned(),
            "ws".to_owned(),
            BTreeMap::from([(Process::Runtime, bad)]),
        );
        assert_eq!(made.err(), Some(SetupError::Template));
    }
}

#[test]
fn in_cluster_refuses_plain_http_and_unreadable_files() {
    let config = |api: &str, token: PathBuf| InCluster {
        api: api.to_owned(),
        token_file: token,
        ca_file: PathBuf::from("/nonexistent/ca.crt"),
        namespace: "cell".to_owned(),
        workspace: "ws".to_owned(),
        templates: BTreeMap::new(),
    };
    let missing = PathBuf::from("/nonexistent/token");
    assert_eq!(
        Kubernetes::in_cluster(config("http://kubernetes.default.svc", missing.clone())).err(),
        Some(SetupError::NotHttps)
    );
    assert_eq!(
        Kubernetes::in_cluster(config("https://kubernetes.default.svc", missing)).err(),
        Some(SetupError::Unreadable)
    );
    let dir = std::env::temp_dir().join(format!("deploy-manager-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let token = dir.join("token");
    std::fs::write(&token, "t\n").unwrap();
    let ca = dir.join("ca.crt");
    std::fs::write(&ca, "not a certificate").unwrap();
    let with_ca = InCluster {
        ca_file: ca,
        ..config("https://kubernetes.default.svc", token)
    };
    assert_eq!(
        Kubernetes::in_cluster(with_ca).err(),
        Some(SetupError::Client)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_adapters_debug_form_names_its_server_and_never_shows_its_token() {
    let shown = format!("{:?}", adapter("http://127.0.0.1:1".to_owned()));
    assert!(!shown.contains("token-xyz"), "{shown}");
    assert!(
        shown.contains("Kubernetes") && shown.contains("127.0.0.1:1") && shown.contains("cell"),
        "{shown}"
    );
}

#[test]
fn an_answer_longer_than_the_bound_is_unavailable() {
    let list = |len: usize| {
        let body = r#"{"items":[]}"#;
        format!("{body}{}", " ".repeat(len - body.len()))
    };
    let (api, _) = server(vec![(200, list(MAX_BODY)), (200, list(MAX_BODY + 1))]);
    let mut k = adapter(api);
    assert_eq!(
        block_on(k.observe()),
        Ok(Vec::new()),
        "exactly the bound is read"
    );
    assert_eq!(block_on(k.observe()), Err(Unavailable));
}

const CONNECTION: &str = "conn_01J8Z2AB";

fn exchange() -> Slot {
    Slot::token_exchange(CONNECTION)
}

/// The account DEC-2060 names for the exchange of [`CONNECTION`]: `x` and its bytes in hex.
fn exchange_account() -> String {
    let hex: String = CONNECTION.bytes().map(|b| format!("{b:02x}")).collect();
    format!("x{hex}")
}

#[test]
fn the_exchanges_account_is_the_one_the_vaults_role_is_named_for() {
    let k = adapter("http://127.0.0.1:1".to_owned());
    let connection = ConnectionId::new(CONNECTION).unwrap();
    assert_eq!(
        k.account_name(&exchange(), None),
        Some(service_account_name(&Identity::TokenExchange(connection)).unwrap())
    );
    assert_eq!(k.account_name(&exchange(), None), Some(exchange_account()));
    assert_eq!(
        k.account_name(&Slot::runtime("agent_a"), None),
        None,
        "a runtime runs as the namespace's default account"
    );
}

#[test]
fn an_exchanges_start_creates_its_account_then_its_pod_ended_by_its_deadline() {
    let (api, seen) = server(vec![(201, "{}".to_owned()), (201, "{}".to_owned())]);
    let mut k = adapter(api);
    let lifetime = Duration::from_millis(599_900);
    assert_eq!(
        block_on(k.start(&exchange(), 1, &launch(Some(lifetime)))),
        Ok(())
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/api/v1/namespaces/cell/serviceaccounts"),
        "the account exists before the pod that names it"
    );
    let account: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(
        account,
        json!({
            "apiVersion": "v1",
            "kind": "ServiceAccount",
            "metadata": {
                "name": exchange_account(),
                "labels": {
                    "app.kubernetes.io/managed-by": "mandate-deploy-manager",
                    "mandate.dev/workspace": "ws1",
                    "mandate.dev/process": "token-exchange"
                }
            },
            "automountServiceAccountToken": false
        }),
        "no token mounted, no Secret, and the labels the accounts policy admits"
    );
    assert_eq!(
        (seen[1].method.as_str(), seen[1].path.as_str()),
        ("POST", "/api/v1/namespaces/cell/pods")
    );
    let pod: Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(
        pod["spec"]["serviceAccountName"],
        exchange_account().as_str()
    );
    assert_eq!(
        pod["spec"]["activeDeadlineSeconds"], 599,
        "whole seconds, rounded down, so the pod never outlives its request"
    );
    assert_eq!(pod["spec"]["restartPolicy"], "Never");
    assert_eq!(
        pod["metadata"]["labels"]["mandate.dev/process"],
        "token-exchange"
    );
    let name = pod["metadata"]["name"].as_str().unwrap();
    assert_eq!(name, k.pod_name(&exchange(), 1));
    assert!(
        name.starts_with("mandate-tx-") && name.ends_with("-g1"),
        "{name}"
    );
    let env: BTreeMap<&str, &str> = pod["spec"]["containers"][0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["name"].as_str().unwrap(), e["value"].as_str().unwrap()))
        .collect();
    assert_eq!(env["MANDATE_PROCESS"], "token_exchange");
    assert_eq!(env["MANDATE_SUBJECT"], CONNECTION);
    assert_eq!(
        env["MANDATE_SUBJECT_ACCOUNT"],
        exchange_account(),
        "its account named beside its connection, which admission and the exchange check"
    );
    assert_eq!(
        pod["spec"]["containers"][0]["command"],
        json!(["/usr/local/bin/mandate-token-exchange"]),
        "the chart's pinned launch is kept"
    );
}

/// The account [`Kubernetes::account`] makes for [`exchange`], as the API server answers a read of it.
fn made_account(k: &Kubernetes) -> Value {
    k.account(&exchange(), None).unwrap()
}

#[test]
fn an_existing_account_is_used_only_if_it_is_the_one_the_manager_makes() {
    let lifetime = Some(Duration::from_secs(300));
    let shape = made_account(&adapter("http://127.0.0.1:1".to_owned()));
    let (api, seen) = server(vec![
        (409, "{}".to_owned()),
        (200, shape.to_string()),
        (201, "{}".to_owned()),
    ]);
    let mut k = adapter(api);
    assert_eq!(block_on(k.start(&exchange(), 1, &launch(lifetime))), Ok(()));
    let asked: Vec<(String, String)> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|s| (s.method.clone(), s.path.clone()))
        .collect();
    assert_eq!(
        asked,
        [
            (
                "POST".to_owned(),
                "/api/v1/namespaces/cell/serviceaccounts".to_owned()
            ),
            (
                "GET".to_owned(),
                format!(
                    "/api/v1/namespaces/cell/serviceaccounts/{}",
                    exchange_account()
                )
            ),
            ("POST".to_owned(), "/api/v1/namespaces/cell/pods".to_owned()),
        ]
    );
    let mut listed_empty = shape.clone();
    listed_empty["secrets"] = json!([]);
    listed_empty["imagePullSecrets"] = json!([]);
    let (api, _) = server(vec![
        (409, "{}".to_owned()),
        (200, listed_empty.to_string()),
        (201, "{}".to_owned()),
    ]);
    assert_eq!(
        block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
        Ok(()),
        "an empty list of Secrets holds none"
    );
    let mut altered: Vec<(&str, Value)> = Vec::new();
    let mut with = |name: &'static str, change: &dyn Fn(&mut Value)| {
        let mut other = shape.clone();
        change(&mut other);
        altered.push((name, other));
    };
    with("a mounted token", &|a| {
        a["automountServiceAccountToken"] = json!(true)
    });
    with("no automount setting", &|a| {
        a.as_object_mut()
            .unwrap()
            .remove("automountServiceAccountToken");
    });
    with("a Secret", &|a| {
        a["secrets"] = json!([{"name": "journal-ca"}])
    });
    with("a pull secret", &|a| {
        a["imagePullSecrets"] = json!([{"name": "s"}])
    });
    with("another manager", &|a| {
        a["metadata"]["labels"]["app.kubernetes.io/managed-by"] = json!("someone-else");
    });
    with("another workspace", &|a| {
        a["metadata"]["labels"]["mandate.dev/workspace"] = json!("ws2")
    });
    with("another process", &|a| {
        a["metadata"]["labels"]["mandate.dev/process"] = json!("executor")
    });
    with("no labels", &|a| {
        a["metadata"].as_object_mut().unwrap().remove("labels");
    });
    for (name, other) in altered {
        let (api, seen) = server(vec![
            (409, "{}".to_owned()),
            (200, other.to_string()),
            (201, "{}".to_owned()),
        ]);
        assert_eq!(
            block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
            Err(Unavailable),
            "{name}"
        );
        assert_eq!(seen.lock().unwrap().len(), 2, "{name}: no pod runs as it");
    }
    for unread in [(404, "{}"), (500, "{}"), (200, "not json")] {
        let (api, seen) = server(vec![
            (409, "{}".to_owned()),
            (unread.0, unread.1.to_owned()),
            (201, "{}".to_owned()),
        ]);
        assert_eq!(
            block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
            Err(Unavailable),
            "{unread:?}"
        );
        assert_eq!(seen.lock().unwrap().len(), 2, "{unread:?}");
    }
}

#[test]
fn a_pod_the_api_server_refused_takes_its_new_account_with_it() {
    let lifetime = Some(Duration::from_secs(300));
    let (api, seen) = server(vec![
        (201, "{}".to_owned()),
        (403, "{}".to_owned()),
        (200, "{}".to_owned()),
    ]);
    assert_eq!(
        block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
        Err(Unavailable)
    );
    let asked: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|s| format!("{} {}", s.method, s.path))
        .collect();
    assert_eq!(
        asked,
        [
            "POST /api/v1/namespaces/cell/serviceaccounts".to_owned(),
            "POST /api/v1/namespaces/cell/pods".to_owned(),
            format!(
                "DELETE /api/v1/namespaces/cell/serviceaccounts/{}",
                exchange_account()
            ),
        ],
        "an account no pod will run as is not left behind"
    );
    for unsure in [500, 503, 429] {
        let (api, seen) = server(vec![
            (201, "{}".to_owned()),
            (unsure, "{}".to_owned()),
            (200, "{}".to_owned()),
        ]);
        assert_eq!(
            block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
            Err(Unavailable)
        );
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "{unsure}: the pod may have been made, so its account is kept for it"
        );
    }
    let (api, seen) = server(vec![(500, "{}".to_owned())]);
    assert_eq!(
        block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
        Err(Unavailable)
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "no account, no pod asked for"
    );
    let shape = made_account(&adapter("http://127.0.0.1:1".to_owned()));
    let (api, seen) = server(vec![
        (409, "{}".to_owned()),
        (200, shape.to_string()),
        (403, "{}".to_owned()),
        (200, "{}".to_owned()),
    ]);
    assert_eq!(
        block_on(adapter(api).start(&exchange(), 1, &launch(lifetime))),
        Err(Unavailable)
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        3,
        "an account that was there before is not this start's to delete"
    );
}

#[test]
fn only_an_exchange_takes_a_lifetime_and_it_always_takes_one_of_a_second_or_more() {
    let k = adapter("http://127.0.0.1:1".to_owned());
    assert_eq!(k.manifest(&exchange(), 1, &launch(None)), None);
    assert_eq!(
        k.manifest(&exchange(), 1, &launch(Some(Duration::from_millis(999)))),
        None,
        "a deadline already reached starts nothing"
    );
    assert!(
        k.manifest(&exchange(), 1, &launch(Some(Duration::from_secs(1))))
            .is_some()
    );
    assert_eq!(
        k.manifest(
            &Slot::runtime("agent_a"),
            1,
            &launch(Some(Duration::from_secs(60)))
        ),
        None,
        "a standing process is never given a deadline"
    );
    let (api, seen) = server(Vec::new());
    let mut k = adapter(api);
    assert_eq!(
        block_on(k.start(&exchange(), 1, &launch(None))),
        Err(Unavailable)
    );
    assert!(seen.lock().unwrap().is_empty(), "nothing is created for it");
}

/// DEC-2363: an exchange's pod names its request's expiry, in the journal's form of a time, so it
/// waits for its code until then and no longer; an exchange without one starts nothing, and no
/// other process is given one.
#[test]
fn an_exchanges_pod_names_its_requests_expiry_and_no_other_process_is_given_one() {
    let k = adapter("http://127.0.0.1:1".to_owned());
    let started = launch(Some(Duration::from_secs(598)));
    let pod = k.manifest(&exchange(), 1, &started).expect("its pod");
    assert_eq!(
        env_of(&pod)
            .get("MANDATE_REQUEST_EXPIRES_AT")
            .map(String::as_str),
        Some("2026-10-17T15:10:10.000000005Z")
    );
    assert_eq!(
        k.manifest(
            &exchange(),
            1,
            &Launch {
                expires_at: None,
                ..started.clone()
            }
        ),
        None,
        "an exchange never starts without its request's expiry"
    );
    let runtime = Slot::runtime("agent_a");
    for (slot, start) in [(&runtime, launch(None)), (&executor(), connected())] {
        let pod = k.manifest(slot, 1, &start).expect("its pod");
        assert!(
            !env_of(&pod).contains_key("MANDATE_REQUEST_EXPIRES_AT"),
            "{slot:?}"
        );
        let given = Launch {
            expires_at: Some(expiry()),
            ..start
        };
        assert_eq!(
            k.manifest(slot, 1, &given),
            None,
            "a standing process is never given an expiry: {slot:?}"
        );
    }
}

#[test]
fn an_exchanges_stop_deletes_its_account_then_its_pod_and_a_failure_keeps_the_pod() {
    let (api, seen) = server(vec![
        (200, "{}".to_owned()),
        (200, "{}".to_owned()),
        (404, "{}".to_owned()),
        (404, "{}".to_owned()),
        (500, "{}".to_owned()),
    ]);
    let mut k = adapter(api);
    assert_eq!(block_on(k.stop(&exchange(), 1, None)), Ok(()));
    assert_eq!(
        block_on(k.stop(&exchange(), 1, None)),
        Ok(()),
        "both already gone"
    );
    assert_eq!(block_on(k.stop(&exchange(), 1, None)), Err(Unavailable));
    let seen = seen.lock().unwrap();
    let asked: Vec<(&str, String)> = seen
        .iter()
        .map(|s| (s.method.as_str(), s.path.clone()))
        .collect();
    let account = (
        "DELETE",
        format!(
            "/api/v1/namespaces/cell/serviceaccounts/{}",
            exchange_account()
        ),
    );
    let pod = (
        "DELETE",
        format!(
            "/api/v1/namespaces/cell/pods/{}",
            k.pod_name(&exchange(), 1)
        ),
    );
    assert_eq!(
        asked,
        [account.clone(), pod.clone(), account.clone(), pod, account],
        "an account it could not delete leaves the pod listed, so the stop is asked again"
    );
}

#[test]
fn an_exchanges_pod_is_listed_by_the_charts_process_name() {
    let list = json!({"items": [
        pod("token-exchange", CONNECTION, "1", "Running", false, false),
    ]});
    let (api, _) = server(vec![(200, list.to_string())]);
    assert_eq!(
        block_on(adapter(api).observe()),
        Ok(vec![Instance {
            slot: exchange(),
            generation: 1,
            phase: Phase::Running { ready: false },
        }])
    );
    let journal_spelling = json!({"items": [
        pod("token_exchange", CONNECTION, "1", "Running", false, false),
    ]});
    let (api, _) = server(vec![(200, journal_spelling.to_string())]);
    assert_eq!(
        block_on(adapter(api).observe()),
        Err(Unavailable),
        "the label is the chart's name, never the journal's code"
    );
}

#[test]
fn each_process_reads_back_from_its_journal_code_and_from_its_chart_label() {
    for process in [Process::Runtime, Process::Executor, Process::TokenExchange] {
        assert_eq!(Process::parse(process.code()), Some(process));
        assert_eq!(Process::from_label(process.label()), Some(process));
    }
    assert_eq!(
        Process::parse("token_exchange"),
        Some(Process::TokenExchange)
    );
    assert_eq!(Process::parse("token-exchange"), None);
    assert_eq!(Process::from_label("token_exchange"), None);
}

const ACCOUNT_REF: &str = "01J8Z2ACCT00000000000000K2";

fn executor() -> Slot {
    Slot::executor(ACCOUNT_REF)
}

/// An executor's start for [`CONNECTION`].
fn connected() -> Launch {
    Launch {
        lifetime: None,
        expires_at: None,
        connection: Some(CONNECTION.to_owned()),
    }
}

/// The account DEC-2060 names for the executor of [`CONNECTION`]: its bytes in hex.
fn executor_account() -> String {
    CONNECTION.bytes().map(|b| format!("{b:02x}")).collect()
}

fn env_of(pod: &Value) -> BTreeMap<String, String> {
    pod["spec"]["containers"][0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["name"].as_str().unwrap().to_owned(),
                e["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn an_executors_account_is_the_one_the_vaults_role_is_named_for() {
    let k = adapter("http://127.0.0.1:1".to_owned());
    let connection = ConnectionId::new(CONNECTION).unwrap();
    assert_eq!(
        k.account_name(&executor(), Some(CONNECTION)),
        Some(service_account_name(&Identity::Executor(connection)).unwrap())
    );
    assert_eq!(
        k.account_name(&executor(), Some(CONNECTION)),
        Some(executor_account())
    );
    assert_eq!(k.account_name(&executor(), None), None);
    assert_eq!(
        k.account_name(&exchange(), Some("conn_other")),
        Some(exchange_account()),
        "an exchange's account is its subject's, whatever it is given"
    );
    assert_eq!(
        k.account_name(&Slot::runtime("agent_a"), Some(CONNECTION)),
        None
    );
}

#[test]
fn an_executors_start_creates_its_connections_account_then_its_pod_running_as_it() {
    let (api, seen) = server(vec![
        (201, "{}".to_owned()),
        (201, "{}".to_owned()),
        (201, "{}".to_owned()),
    ]);
    let mut k = adapter(api);
    assert_eq!(block_on(k.start(&executor(), 2, &connected())), Ok(()));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "one account, one pod");
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/api/v1/namespaces/cell/serviceaccounts")
    );
    let account: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(account, k.account(&executor(), Some(CONNECTION)).unwrap());
    assert_eq!(account["metadata"]["name"], executor_account().as_str());
    assert_eq!(
        account["metadata"]["labels"]["mandate.dev/process"],
        "executor"
    );
    assert_eq!(account["automountServiceAccountToken"], false);
    let pod: Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(
        pod["spec"]["serviceAccountName"],
        executor_account().as_str()
    );
    assert!(
        pod["spec"].get("activeDeadlineSeconds").is_none(),
        "a standing process has no deadline"
    );
    let env = env_of(&pod);
    assert_eq!(env["MANDATE_SUBJECT"], ACCOUNT_REF);
    assert_eq!(env["MANDATE_SUBJECT_ACCOUNT"], executor_account());
    assert_eq!(
        env["MANDATE_WORKSPACE_NAMESPACE"], "cell",
        "the namespace its vault token is bound to, which it checks"
    );
    assert_eq!(env.len(), 7, "the template's one, and the manager's six");
    let runtime = k
        .manifest(&Slot::runtime("agent_a"), 1, &launch(None))
        .unwrap();
    assert!(runtime["spec"].get("serviceAccountName").is_none());
    assert!(!env_of(&runtime).contains_key("MANDATE_WORKSPACE_NAMESPACE"));
    assert!(!env_of(&runtime).contains_key("MANDATE_SUBJECT_ACCOUNT"));
}

#[test]
fn an_executor_without_its_connection_starts_nothing() {
    let k = adapter("http://127.0.0.1:1".to_owned());
    assert!(k.manifest(&executor(), 1, &connected()).is_some());
    assert_eq!(k.manifest(&executor(), 1, &launch(None)), None);
    let (api, seen) = server(vec![(201, "{}".to_owned())]);
    let mut k = adapter(api);
    assert_eq!(
        block_on(k.start(&executor(), 1, &launch(None))),
        Err(Unavailable)
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "no account, no pod asked for"
    );
}

#[test]
fn an_executors_stop_deletes_its_account_then_its_pod() {
    let (api, seen) = server(vec![
        (200, "{}".to_owned()),
        (200, "{}".to_owned()),
        (500, "{}".to_owned()),
        (200, "{}".to_owned()),
    ]);
    let mut k = adapter(api);
    assert_eq!(block_on(k.stop(&executor(), 3, Some(CONNECTION))), Ok(()));
    assert_eq!(
        block_on(k.stop(&executor(), 3, Some(CONNECTION))),
        Err(Unavailable)
    );
    let asked: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|s| format!("{} {}", s.method, s.path))
        .collect();
    let account = format!(
        "DELETE /api/v1/namespaces/cell/serviceaccounts/{}",
        executor_account()
    );
    let pod = format!(
        "DELETE /api/v1/namespaces/cell/pods/{}",
        k.pod_name(&executor(), 3)
    );
    assert_eq!(
        asked,
        [account.clone(), pod, account],
        "an account it could not delete leaves the pod listed, so the stop is asked again"
    );
}

#[test]
fn a_succeeded_pod_reads_as_succeeded_and_a_failed_one_as_exited() {
    let list = json!({"items": [
        pod("token-exchange", CONNECTION, "1", "Succeeded", false, false),
        pod("token-exchange", "conn_failed", "1", "Failed", false, false),
        pod("executor", ACCOUNT_REF, "1", "Succeeded", false, true),
    ]});
    let (api, _) = server(vec![(200, list.to_string())]);
    let phases: Vec<(Slot, Phase)> = block_on(adapter(api).observe())
        .unwrap()
        .into_iter()
        .map(|i| (i.slot, i.phase))
        .collect();
    assert_eq!(
        phases,
        [
            (exchange(), Phase::Succeeded),
            (Slot::token_exchange("conn_failed"), Phase::Exited),
            (executor(), Phase::Succeeded),
        ],
        "an exit's status holds whether or not its pod is being deleted"
    );
}
