//! The Kubernetes adapter: plain HTTPS to the cluster's API server through `reqwest` and `rustls`,
//! no Kubernetes client crate (D-3, DEC-990 item 1). Each instance is a bare pod with
//! `restartPolicy: Never`, so only the manager restarts a process and every restart is journaled
//! (DEC-1170 item 2). The pod's shape (image, probes, resources, affinity, the labels the network
//! policies select) is the chart's template; the adapter adds only the pod's name, its labels and
//! the process's identity. A token exchange also runs as its connection's own ServiceAccount, which
//! the adapter makes just before the pod and deletes just before deleting it, and ends by its request's
//! deadline (DEC-1175 item 4, DEC-1177 items 4, 6 and 11).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use mandate_canon::Digest;
use reqwest::{Client, Method, StatusCode};
use serde_json::{Map, Value, json};

use crate::control::{Process, Slot};
use crate::orchestrator::{Instance, Launch, Orchestrator, Phase, Unavailable};

const MANAGED_BY: &str = "app.kubernetes.io/managed-by";
const MANAGER: &str = "mandate-deploy-manager";
/// The manager's own key for its workspace. The chart's `mandate.dev/workspace` label, which the
/// network policies and the colocation affinity select, is the template's and is kept as it is.
const WORKSPACE: &str = "mandate.dev/workspace-key";
const PROCESS: &str = "mandate.dev/process";
/// The chart's workspace label, copied from the template onto an account it makes.
const CHART_WORKSPACE: &str = "mandate.dev/workspace";
const GENERATION: &str = "mandate.dev/generation";
const SUBJECT: &str = "mandate.dev/subject";
/// The subject as a selectable label: an id is not always label-safe, so the label is its key.
const SUBJECT_KEY: &str = "mandate.dev/subject-key";
/// The largest answer read from the API server; a longer one is unavailable.
pub const MAX_BODY: usize = 8_388_608;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Why the in-cluster adapter could not be built. None carries a path's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupError {
    /// The API server's address is not `https://`.
    NotHttps,
    /// The service account's token or CA file could not be read.
    Unreadable,
    /// The CA file holds no certificate, or the client could not be built.
    Client,
    /// A pod template is not a JSON object with a `spec` holding `containers`.
    Template,
}

/// How the manager reaches the API server from inside the cell: the service account's files,
/// mounted read-only, never an environment variable (DEC-822 item 4).
#[derive(Debug, Clone)]
pub struct InCluster {
    /// `https://kubernetes.default.svc` in a pod.
    pub api: String,
    pub token_file: PathBuf,
    pub ca_file: PathBuf,
    pub namespace: String,
    pub workspace: String,
    /// The chart's pod template for each process.
    pub templates: BTreeMap<Process, Value>,
}

/// The adapter. Its token is never printed.
pub struct Kubernetes {
    client: Client,
    api: String,
    namespace: String,
    token: String,
    workspace: String,
    templates: BTreeMap<Process, Value>,
}

impl std::fmt::Debug for Kubernetes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kubernetes")
            .field("api", &self.api)
            .field("namespace", &self.namespace)
            .finish_non_exhaustive()
    }
}

impl Kubernetes {
    /// The adapter inside the cell: HTTPS only, the cluster's CA only, no redirect, no proxy.
    ///
    /// # Errors
    /// [`SetupError`].
    pub fn in_cluster(config: InCluster) -> Result<Self, SetupError> {
        if !config.api.starts_with("https://") {
            return Err(SetupError::NotHttps);
        }
        let token = std::fs::read_to_string(&config.token_file)
            .map_err(|_| SetupError::Unreadable)?
            .trim()
            .to_owned();
        let ca = std::fs::read(&config.ca_file).map_err(|_| SetupError::Unreadable)?;
        let certificates =
            reqwest::Certificate::from_pem_bundle(&ca).map_err(|_| SetupError::Client)?;
        if certificates.is_empty() {
            return Err(SetupError::Client);
        }
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .https_only(true)
            .tls_certs_only(certificates)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| SetupError::Client)?;
        Self::with_client(
            client,
            config.api,
            config.namespace,
            token,
            config.workspace,
            config.templates,
        )
    }

    /// The adapter over a given client and API address, as the tests build it on loopback.
    ///
    /// # Errors
    /// [`SetupError::Template`] for a template without `spec.containers`.
    pub fn with_client(
        client: Client,
        api: String,
        namespace: String,
        token: String,
        workspace: String,
        templates: BTreeMap<Process, Value>,
    ) -> Result<Self, SetupError> {
        let shaped = templates.values().all(|t| {
            t.get("spec")
                .and_then(|s| s.get("containers"))
                .and_then(Value::as_array)
                .is_some_and(|c| c.iter().all(Value::is_object))
        });
        if !shaped {
            return Err(SetupError::Template);
        }
        Ok(Self {
            client,
            api: api.trim_end_matches('/').to_owned(),
            namespace,
            token,
            workspace,
            templates,
        })
    }

    /// A pod's name: DNS-safe whatever the subject's spelling, one per slot and generation.
    pub fn pod_name(&self, slot: &Slot, generation: u64) -> String {
        let short = match slot.process {
            Process::Runtime => "rt",
            Process::Executor => "ex",
            Process::TokenExchange => "tx",
        };
        let key = format!("{}:{}", self.workspace, slot.subject);
        format!("mandate-{short}-{}-g{generation}", hex16(&key))
    }

    /// The ServiceAccount `slot` runs as, when it is not the namespace's default: a token
    /// exchange's is `x` and its connection id's bytes in lowercase hex, the name its vault role
    /// and the admission policies are written for (DEC-2060). An executor's `connection` is the
    /// one it acts for.
    pub fn account_name(&self, slot: &Slot, connection: Option<&str>) -> Option<String> {
        match slot.process {
            Process::TokenExchange => Some(format!("x{}", lower_hex(&slot.subject))),
            Process::Executor => connection.map(lower_hex),
            Process::Runtime => None,
        }
    }

    /// The account [`Kubernetes::account_name`] names for `slot`: no token mounted, no Secret, and
    /// the manager's, the chart's workspace and the process's labels, which the chart's accounts
    /// policy requires.
    pub fn account(&self, slot: &Slot, connection: Option<&str>) -> Option<Value> {
        let name = self.account_name(slot, connection)?;
        let workspace = self
            .templates
            .get(&slot.process)?
            .get("metadata")?
            .get("labels")?
            .get(CHART_WORKSPACE)?
            .clone();
        Some(json!({
            "apiVersion": "v1",
            "kind": "ServiceAccount",
            "metadata": {
                "name": name,
                "labels": {
                    MANAGED_BY: MANAGER,
                    CHART_WORKSPACE: workspace,
                    PROCESS: slot.process.label(),
                }
            },
            "automountServiceAccountToken": false
        }))
    }

    /// The pod `generation` of `slot` is created from: the chart's template with the name, the
    /// labels, the subject, `restartPolicy: Never`, and the process's identity in its environment.
    /// A token exchange also names its account, as `serviceAccountName` and, beside its
    /// connection, as `MANDATE_SUBJECT_ACCOUNT`, which admission and the exchange check against
    /// each other (DEC-1177 item 6), and ends by its `lifetime`, in whole seconds
    /// rounded down, so it never outlives its request; it needs one of at least a second, and a
    /// standing process is never given one.
    pub fn manifest(&self, slot: &Slot, generation: u64, launch: &Launch) -> Option<Value> {
        let deadline = match (slot.process, launch.lifetime) {
            (Process::TokenExchange, Some(lifetime)) if lifetime.as_secs() >= 1 => {
                Some(lifetime.as_secs())
            }
            (Process::Runtime | Process::Executor, None) => None,
            _ => return None,
        };
        let mut pod = self.templates.get(&slot.process)?.clone();
        let root = pod.as_object_mut()?;
        root.insert("apiVersion".to_owned(), json!("v1"));
        root.insert("kind".to_owned(), json!("Pod"));
        let metadata = member(root, "metadata")?;
        metadata.insert("name".to_owned(), json!(self.pod_name(slot, generation)));
        let labels = member(metadata, "labels")?;
        labels.insert(MANAGED_BY.to_owned(), json!(MANAGER));
        labels.insert(WORKSPACE.to_owned(), json!(hex16(&self.workspace)));
        labels.insert(PROCESS.to_owned(), json!(slot.process.label()));
        labels.insert(GENERATION.to_owned(), json!(generation.to_string()));
        labels.insert(SUBJECT_KEY.to_owned(), json!(hex16(&slot.subject)));
        member(metadata, "annotations")?.insert(SUBJECT.to_owned(), json!(slot.subject));
        let spec = member(root, "spec")?;
        spec.insert("restartPolicy".to_owned(), json!("Never"));
        if let Some(seconds) = deadline {
            spec.insert("activeDeadlineSeconds".to_owned(), json!(seconds));
        }
        let account = self.account_name(slot, launch.connection.as_deref());
        if slot.process == Process::Executor && account.is_none() {
            return None;
        }
        if let Some(account) = &account {
            spec.insert("serviceAccountName".to_owned(), json!(account));
        }
        let mut env = vec![
            ("MANDATE_WORKSPACE", self.workspace.clone()),
            ("MANDATE_PROCESS", slot.process.code().to_owned()),
            ("MANDATE_SUBJECT", slot.subject.clone()),
            ("MANDATE_GENERATION", generation.to_string()),
        ];
        env.extend(account.map(|account| ("MANDATE_SUBJECT_ACCOUNT", account)));
        if slot.process == Process::Executor {
            env.push(("MANDATE_WORKSPACE_NAMESPACE", self.namespace.clone()));
        }
        for container in spec.get_mut("containers")?.as_array_mut()? {
            let vars = container
                .as_object_mut()?
                .entry("env")
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()?;
            for (name, value) in &env {
                vars.push(json!({"name": name, "value": value}));
            }
        }
        Some(pod)
    }

    fn pods(&self) -> String {
        format!("{}/api/v1/namespaces/{}/pods", self.api, self.namespace)
    }

    fn accounts(&self) -> String {
        format!(
            "{}/api/v1/namespaces/{}/serviceaccounts",
            self.api, self.namespace
        )
    }

    /// Makes `slot`'s account, `true`, or finds the one already made, `false`; an account of that
    /// name the manager would not have made (another manager's, a mounted token, a Secret) is
    /// refused, so no pod runs as it (DEC-2332 item 1).
    async fn make_account(
        &self,
        slot: &Slot,
        connection: Option<&str>,
    ) -> Result<bool, Unavailable> {
        let made = self.account(slot, connection).ok_or(Unavailable)?;
        let (status, _) = self
            .send(Method::POST, self.accounts(), Some(&made))
            .await?;
        if status.is_success() {
            return Ok(true);
        }
        let name = self.account_name(slot, connection).ok_or(Unavailable)?;
        if status != StatusCode::CONFLICT {
            return Err(Unavailable);
        }
        let (status, body) = self
            .send(Method::GET, format!("{}/{name}", self.accounts()), None)
            .await?;
        let found: Value = serde_json::from_slice(&body).map_err(|_| Unavailable)?;
        if status == StatusCode::OK && same_account(&found, &made) {
            Ok(false)
        } else {
            Err(Unavailable)
        }
    }

    /// Deletes `url`; one already gone is deleted.
    async fn delete(&self, url: String) -> Result<(), Unavailable> {
        let (status, _) = self.send(Method::DELETE, url, None).await?;
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(Unavailable)
        }
    }

    async fn send(
        &self,
        method: Method,
        url: String,
        body: Option<&Value>,
    ) -> Result<(StatusCode, Vec<u8>), Unavailable> {
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(&self.token)
            .header("accept", "application/json");
        if let Some(body) = body {
            let bytes = serde_json::to_vec(body).map_err(|_| Unavailable)?;
            request = request
                .header("content-type", "application/json")
                .body(bytes);
        }
        let mut response = request.send().await.map_err(|_| Unavailable)?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Unavailable)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err(Unavailable);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, bytes))
    }
}

impl Orchestrator for Kubernetes {
    async fn observe(&mut self) -> Result<Vec<Instance>, Unavailable> {
        let selector = format!(
            "{MANAGED_BY}={MANAGER},{WORKSPACE}={}",
            hex16(&self.workspace)
        );
        let url = reqwest::Url::parse_with_params(&self.pods(), [("labelSelector", selector)])
            .map_err(|_| Unavailable)?;
        let (status, body) = self.send(Method::GET, url.to_string(), None).await?;
        if status != StatusCode::OK {
            return Err(Unavailable);
        }
        let list: Value = serde_json::from_slice(&body).map_err(|_| Unavailable)?;
        list.get("items")
            .and_then(Value::as_array)
            .ok_or(Unavailable)?
            .iter()
            .map(instance)
            .collect()
    }

    async fn start(
        &mut self,
        slot: &Slot,
        generation: u64,
        launch: &Launch,
    ) -> Result<(), Unavailable> {
        let pod = self.manifest(slot, generation, launch).ok_or(Unavailable)?;
        let connection = launch.connection.as_deref();
        let account = self.account_name(slot, connection);
        let made = match account {
            Some(_) => self.make_account(slot, connection).await?,
            None => false,
        };
        let (status, _) = self.send(Method::POST, self.pods(), Some(&pod)).await?;
        if status.is_success() || status == StatusCode::CONFLICT {
            return Ok(());
        }
        if let Some(account) = account
            && made
            && refused(status)
        {
            let _swept_again_when_the_request_closes =
                self.delete(format!("{}/{account}", self.accounts())).await;
        }
        Err(Unavailable)
    }

    /// Deletes the account first, then the pod: a stop that fails leaves the pod listed, so the
    /// manager asks again, and no account outlives the pod it was made for.
    async fn stop(
        &mut self,
        slot: &Slot,
        generation: u64,
        connection: Option<&str>,
    ) -> Result<(), Unavailable> {
        if let Some(account) = self.account_name(slot, connection) {
            self.delete(format!("{}/{account}", self.accounts()))
                .await?;
        }
        let pod = format!("{}/{}", self.pods(), self.pod_name(slot, generation));
        self.delete(pod).await
    }
}

/// Whether the API server refused a create outright: a client error other than a conflict or a
/// rate limit, so nothing was made. A server error or a lost answer may have made it.
fn refused(status: StatusCode) -> bool {
    status.is_client_error()
        && status != StatusCode::CONFLICT
        && status != StatusCode::TOO_MANY_REQUESTS
}

/// Whether `found` is the account the manager makes, `made`: its labels, no mounted token, and
/// no Secret or pull secret.
fn same_account(found: &Value, made: &Value) -> bool {
    let label =
        |account: &Value, name: &str| account.get("metadata")?.get("labels")?.get(name).cloned();
    let labelled = [MANAGED_BY, CHART_WORKSPACE, PROCESS]
        .iter()
        .all(|name| label(found, name).is_some() && label(found, name) == label(made, name));
    let empty = |member: &str| {
        found
            .get(member)
            .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
    };
    labelled
        && found.get("automountServiceAccountToken") == Some(&Value::Bool(false))
        && empty("secrets")
        && empty("imagePullSecrets")
}

/// One listed pod as an instance. A pod of this manager that cannot be read makes the whole list
/// unreadable, so no step is decided while an instance might be hidden.
fn instance(pod: &Value) -> Result<Instance, Unavailable> {
    let metadata = pod.get("metadata").ok_or(Unavailable)?;
    let label = |name: &str| metadata.get("labels")?.get(name)?.as_str();
    let process = label(PROCESS)
        .and_then(Process::from_label)
        .ok_or(Unavailable)?;
    let generation = label(GENERATION)
        .and_then(|g| g.parse::<u64>().ok())
        .ok_or(Unavailable)?;
    let subject = metadata
        .get("annotations")
        .and_then(|a| a.get(SUBJECT))
        .and_then(Value::as_str)
        .ok_or(Unavailable)?;
    let status = pod.get("status");
    let ready = status
        .and_then(|s| s.get("conditions"))
        .and_then(Value::as_array)
        .is_some_and(|conditions| {
            conditions.iter().any(|c| {
                c.get("type").and_then(Value::as_str) == Some("Ready")
                    && c.get("status").and_then(Value::as_str) == Some("True")
            })
        });
    let deleting = metadata
        .get("deletionTimestamp")
        .is_some_and(|d| !d.is_null());
    let phase = match status.and_then(|s| s.get("phase")).and_then(Value::as_str) {
        Some("Succeeded") => Phase::Succeeded,
        Some("Failed") => Phase::Exited,
        Some("Pending") if !deleting => Phase::Pending,
        Some("Running") if !deleting => Phase::Running { ready },
        _ => Phase::Unknown,
    };
    Ok(Instance {
        slot: Slot {
            process,
            subject: subject.to_owned(),
        },
        generation,
        phase,
    })
}

/// The object member `name` of `object`, created empty when absent; `None` when it is not an
/// object.
fn member<'a>(
    object: &'a mut Map<String, Value>,
    name: &str,
) -> Option<&'a mut Map<String, Value>> {
    object
        .entry(name.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
}

/// `text`'s bytes in lowercase hex, two digits each.
fn lower_hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// The first 16 hex digits of the SHA-256 of `text`: a label-safe, DNS-safe key.
fn hex16(text: &str) -> String {
    Digest::of(text.as_bytes())
        .to_hex()
        .chars()
        .take(16)
        .collect()
}
