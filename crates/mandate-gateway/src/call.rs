//! The call contract (inference spec §3, E15-6): one request in, exactly one [`CallRecord`] out,
//! with a validated output of the pinned model only when the outcome is `ok`.
//!
//! The order of a call:
//! 1. **Refusals, nothing sent:** the workspace, the model's registration and withdrawal
//!    (`policy_denied`, `model_withdrawn`), the input contract and the prompt guard
//!    (`input_rejected`, the prompt never stored), an endpoint the policy and the egress allowlist
//!    both allow (`policy_denied`), the exact cache, the local rate caps (`rate_limited_local`),
//!    and the meter's reservation of the maximum cost (`budget_exhausted`, `meter_unavailable`).
//! 2. **Attempts,** only while the deadline leaves time, at most [`MAX_ATTEMPTS`]: a transport
//!    failure retries, a content failure never does (inference spec §3.4), and neither does an
//!    attempt that was billed or may have been. A response read after the deadline is
//!    `deadline_exceeded`, discarded, and never cached (INF-3). A 200 whose reported model or
//!    provider is not the pin's, or is missing, is `identity_mismatch` whatever the rest of its
//!    shape, and the endpoint is marked unhealthy (INF-2).
//! 3. **Settlement** of the reservation at the reported cost, or at the reservation's maximum when
//!    the cost of an attempt cannot be read, so spend is never under-counted.
//!
//! **A turn of an agentic run** (inference spec v0.4 §3.1, §3.2; DEC-2203) is one call too. Its
//! request names the run's kind, offers some of the entry's pinned tools, and hands back the
//! transcript of the run's earlier turns, each reply by its bytes and each tool result as text; a
//! request the entry could not have made (a tool it does not declare for that kind, a transcript
//! that is not a run of its validated tool calls) is a [`RequestError`]. Each result goes into its
//! own data section and through the guard, as every input does. A reply with any tool call is a
//! tool-call reply: each call must name an offered tool with arguments its schema accepts, at most
//! `max_tool_calls_per_turn` of them, or the call is `schema_invalid`. The gateway runs none of
//! them; it returns them, validated, as data. A turn's reservation must also fit under the request's
//! `spend_ceiling_usd`, the run's ceiling less what its earlier turns cost.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, is_ulid, parse, to_canonical};
use mandate_time::{UtcNanos, new_york_date_and_hour};

use crate::guard::{self, GuardContext, GuardRule};
use crate::kinds::{KeyOwner, Outcome, Purpose};
use crate::meter::{Meter, MeterRefusal, MeterStore, Reservation, Settlement};
use crate::money::{Usage, Usd6};
use crate::policy::{EgressAllowlist, EndpointPolicy};
use crate::provider::{
    Clock, Delivery, HttpRequest, IdSource, Prompt, ProviderAdapter, Reply, SecretSource,
    Transport, after_ms, before_ms,
};
use crate::record::CallRecord;
use crate::registry::{
    Endpoint, Entry, Locality, ModelRef, Registry, RegistryError, Status, Template, Unresolved,
    int, object,
};
use crate::schema;
use crate::tools::{self, ToolCall, ToolDeclaration, TranscriptTurn, Turn};

/// The most attempts one call makes (inference spec §3.4: retries only within the deadline).
pub const MAX_ATTEMPTS: u32 = 3;
/// How long a provider's 429 is waited out when it names no wait.
pub const DEFAULT_RETRY_AFTER_MS: u64 = 1_000;
/// The window the local rate caps count over.
pub const RATE_WINDOW_MS: u64 = 60_000;

/// One logical call (inference spec §3.1). Callers pass typed inputs, never prompt text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRequest {
    pub call_id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub purpose: Purpose,
    pub model: ModelRef,
    pub inputs: BTreeMap<String, String>,
    pub as_of: UtcNanos,
    pub deadline: UtcNanos,
    pub observations: Vec<String>,
    pub key_owner: KeyOwner,
    /// The agentic run's kind, which picks the final output's schema; `None` for a `single_call`
    /// entry.
    pub run_kind: Option<String>,
    /// The run's earlier turns, oldest first; empty for a run's first turn and for a `single_call`
    /// entry.
    pub turns: Vec<Turn>,
    /// The names of the pinned tools this turn offers; empty for a `single_call` entry.
    pub tools: Vec<String>,
    /// The most this call may reserve: the run's ceiling less its earlier turns' cost (inference
    /// spec §7.3). Not part of the canonical request.
    pub spend_ceiling: Option<Usd6>,
}

/// The gateway's own rate caps, per workspace and per agent, in calls per minute (inference spec
/// §7.3); a call over either is refused before it reserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateCaps {
    pub per_workspace: u32,
    pub per_agent: u32,
}

/// What a workspace's gateway is configured with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub workspace_id: String,
    pub policy: EndpointPolicy,
    pub guard: GuardContext,
    pub rate: RateCaps,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CacheKey {
    workspace: String,
    content_hash: Digest,
    request_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cached {
    output: Option<Value>,
    tool_calls: Vec<ToolCall>,
    response_ref: Digest,
    reported_identity: Option<String>,
}

/// What a request asks of its entry, checked before the call starts: the tools offered in this
/// turn, in the entry's order; the transcript, each turn's calls validated and its results as the
/// caller gave them; the final output's schema; and the bound on one reply's calls.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    offered: Vec<ToolDeclaration>,
    transcript: Vec<TranscriptTurn>,
    output_schema: Value,
    per_turn: u32,
}

/// One workspace's model gateway: the only component that calls a model (INF-13: nothing on the
/// trading hot path waits on it).
pub struct Gateway<T, S, C> {
    settings: Settings,
    registry: Registry,
    adapter: Box<dyn ProviderAdapter>,
    transport: T,
    secrets: Box<dyn SecretSource>,
    meter: Meter<S>,
    clock: C,
    ids: Box<dyn IdSource>,
    cache: BTreeMap<CacheKey, Cached>,
    admitted: Vec<(UtcNanos, Option<String>)>,
    orphans: u64,
}

/// What a call has gathered so far, turned into its record at the end.
struct Draft {
    record: CallRecord,
    plan: Plan,
}

impl Draft {
    fn finish(mut self, outcome: Outcome, at: UtcNanos) -> CallRecord {
        self.record.outcome = outcome;
        self.record.completed_at = at;
        if outcome != Outcome::Ok {
            self.record.output = None;
            self.record.tool_calls.clear();
        }
        self.record
    }
}

/// How one attempt ended.
enum Attempt {
    Output {
        output: Option<Value>,
        tool_calls: Vec<ToolCall>,
        reply_ref: Digest,
        body: Vec<u8>,
    },
    Failed(Outcome),
    Retry(Outcome),
}

impl<T: Transport, S: MeterStore, C: Clock> Gateway<T, S, C> {
    #[expect(
        clippy::too_many_arguments,
        reason = "each seam is its own input; a builder adds a type for no rule"
    )]
    pub fn new(
        settings: Settings,
        registry: Registry,
        adapter: Box<dyn ProviderAdapter>,
        transport: T,
        secrets: Box<dyn SecretSource>,
        meter: Meter<S>,
        clock: C,
        ids: Box<dyn IdSource>,
    ) -> Self {
        Self {
            settings,
            registry,
            adapter,
            transport,
            secrets,
            meter,
            clock,
            ids,
            cache: BTreeMap::new(),
            admitted: Vec::new(),
            orphans: 0,
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn meter(&self) -> &Meter<S> {
        &self.meter
    }

    pub fn meter_mut(&mut self) -> &mut Meter<S> {
        &mut self.meter
    }

    /// The calls refused `meter_unavailable` that left an orphan open on the deployment's meter
    /// stream (DEC-2207 item 4): the count the operator's alert reads.
    pub fn orphans(&self) -> u64 {
        self.orphans
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn clock_mut(&mut self) -> &mut C {
        &mut self.clock
    }

    /// Whether the exact cache holds an entry for this request digest; never for a call that has
    /// none (a refused prompt).
    pub fn cached(&self, content_hash: Digest, request_digest: Option<Digest>) -> bool {
        request_digest.is_some_and(|request_digest| {
            self.cache.contains_key(&CacheKey {
                workspace: self.settings.workspace_id.clone(),
                content_hash,
                request_digest,
            })
        })
    }

    /// The prompt guard on its own: no call, no reservation, no cost (inference spec §8.3).
    pub fn guard(&self, text: &str) -> Result<(), GuardRule> {
        guard::check(text, &self.settings.guard)
    }

    /// Withdraws a model (INF-15): its calls are refused from now on and its cache index entries
    /// are dropped. Nothing replaces it.
    pub fn withdraw(&mut self, model: &ModelRef) -> Result<(), RegistryError> {
        self.registry.set_status(model, Status::Withdrawn)?;
        self.cache
            .retain(|key, _| key.content_hash != model.content_hash);
        Ok(())
    }

    /// Restores or removes an endpoint's health (an operator's act, inference spec §9).
    pub fn set_endpoint_healthy(
        &mut self,
        model: &ModelRef,
        endpoint_id: &str,
        healthy: bool,
    ) -> Result<(), RegistryError> {
        self.registry.set_healthy(model, endpoint_id, healthy)
    }

    /// One call, start to end. A request that is not a call this gateway can make at all (another
    /// workspace's, a malformed call id, an agent and purpose that disagree, or a pin the registry
    /// does not hold) is refused as a [`RequestError`] with no record: it has no template to render,
    /// so it could not be journaled under journal spec rule 148, and it sends, reserves, and costs
    /// nothing. Every other request gets exactly one [`CallRecord`].
    pub fn call(&mut self, request: &CallRequest) -> Result<CallRecord, RequestError> {
        let start = self.clock.now();
        if request.workspace_id != self.settings.workspace_id {
            return Err(RequestError::ForeignWorkspace);
        }
        if request.agent_id.is_some() != request.purpose.names_agent() {
            return Err(RequestError::AgentPurpose);
        }
        if !is_ulid(&request.call_id) {
            return Err(RequestError::CallId);
        }
        let entry = match self.registry.entry(&request.model) {
            Ok(entry) => entry.clone(),
            Err(Unresolved::HashMismatch) => return Err(RequestError::HashMismatch),
            Err(Unresolved::Unknown | Unresolved::Withdrawn) => {
                return Err(RequestError::UnknownModel);
            }
        };
        let plan = self.plan(request, &entry)?;
        Ok(self.call_entry(request, entry, plan, start))
    }

    /// What the request asks of `entry`, or why the entry could not have been asked it.
    fn plan(&self, request: &CallRequest, entry: &Entry) -> Result<Plan, RequestError> {
        let content = entry.content();
        let Some(agentic) = content.agentic() else {
            if request.run_kind.is_some() || !request.turns.is_empty() || !request.tools.is_empty()
            {
                return Err(RequestError::NotAgentic);
            }
            return Ok(Plan {
                offered: Vec::new(),
                transcript: Vec::new(),
                output_schema: content.output_schema.clone(),
                per_turn: 0,
            });
        };
        let kind = request.run_kind.as_deref().ok_or(RequestError::RunKind)?;
        let output_schema = agentic
            .output_schemas
            .get(kind)
            .cloned()
            .ok_or(RequestError::RunKind)?;
        let menu: Vec<&ToolDeclaration> = agentic
            .tools
            .iter()
            .filter(|t| t.menu.iter().any(|k| k == kind))
            .collect();
        let named = |name: &String| request.tools.iter().filter(|n| *n == name).count();
        if request
            .tools
            .iter()
            .any(|name| named(name) != 1 || !menu.iter().any(|t| t.name == *name))
        {
            return Err(RequestError::Tools);
        }
        let offered = menu
            .iter()
            .filter(|t| request.tools.contains(&t.name))
            .map(|t| (*t).clone())
            .collect();
        let per_turn = agentic.loop_bounds.max_tool_calls_per_turn;
        if u32::try_from(request.turns.len()).map_or(true, |n| n >= agentic.loop_bounds.max_turns) {
            return Err(RequestError::Transcript);
        }
        let transcript = request
            .turns
            .iter()
            .map(|turn| {
                let reply = self
                    .adapter
                    .reply(&turn.response)
                    .ok_or(RequestError::Transcript)?;
                let calls = tools::validate(&reply.tool_calls, &menu, per_turn)
                    .map_err(|_| RequestError::Transcript)?;
                if calls.is_empty() || calls.len() != turn.results.len() {
                    return Err(RequestError::Transcript);
                }
                Ok(TranscriptTurn {
                    calls,
                    results: turn.results.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Plan {
            offered,
            transcript,
            output_schema,
            per_turn,
        })
    }

    fn call_entry(
        &mut self,
        request: &CallRequest,
        entry: Entry,
        plan: Plan,
        start: UtcNanos,
    ) -> CallRecord {
        let mut draft = Draft {
            record: skeleton(request, start),
            plan,
        };
        let content = entry.content();
        let latest = after_ms(start, content.deadline_ms).unwrap_or(start);
        draft.record.deadline = request.deadline.min(latest);
        draft.record.seed = content.seed;
        let Some(mut prompt) = render(&content.template, &request.inputs) else {
            draft.record.guard_rule = Some(GuardRule::InputContract);
            return draft.finish(Outcome::InputRejected, start);
        };
        let Some(transcript) = spotlight(&draft.plan.transcript) else {
            draft.record.guard_rule = Some(GuardRule::InputContract);
            return draft.finish(Outcome::InputRejected, start);
        };
        prompt.tools = draft.plan.offered.clone();
        prompt.transcript = transcript;
        let verdict = guard::check(&prompt.system, &self.settings.guard)
            .and_then(|()| guard::check(&prompt.user, &self.settings.guard))
            .and_then(|()| {
                prompt
                    .transcript
                    .iter()
                    .flat_map(|turn| &turn.results)
                    .try_for_each(|result| guard::check(result, &self.settings.guard))
            })
            .and_then(|()| {
                prompt
                    .transcript
                    .iter()
                    .flat_map(|turn| &turn.calls)
                    .try_for_each(|call| {
                        guard::check(
                            &tools::canonical_text(&call.arguments),
                            &self.settings.guard,
                        )
                    })
            });
        if let Err(rule) = verdict {
            draft.record.guard_rule = Some(rule);
            return draft.finish(Outcome::InputRejected, start);
        }
        let prompt_bytes = prompt_bytes(&prompt);
        let digest = request_digest_for(
            &entry,
            &prompt,
            request.as_of,
            request.run_kind.as_deref(),
            &draft.plan.output_schema,
        );
        draft.record.prompt_ref = Some(Digest::of(&prompt_bytes));
        draft.record.request_digest = Some(digest);
        draft.record.prompt = Some(prompt_bytes.clone());
        if entry.status() == Status::Withdrawn {
            return draft.finish(Outcome::ModelWithdrawn, start);
        }
        let allowlist = EgressAllowlist::of(&self.registry, &self.settings.policy);
        let Some(endpoint) = self.endpoint(&entry, &allowlist) else {
            return draft.finish(Outcome::PolicyDenied, start);
        };
        draft.record.endpoint = endpoint.id.clone();
        draft.record.price_table_ref = endpoint.prices.version();
        let http = self.adapter.request(entry.content(), &endpoint, &prompt);
        if !allowlist.permits(&http.host) {
            return draft.finish(Outcome::PolicyDenied, start);
        }
        if start < draft.record.deadline
            && let Some(record) = self.cache_hit(&mut draft, &entry, digest, start)
        {
            return record;
        }
        if !self.within_rate(request.agent_id.as_deref(), start) {
            return draft.finish(Outcome::RateLimitedLocal, start);
        }
        let input_bound = prompt_bytes.len().max(http.body.len());
        let Ok(max_cost) = endpoint.prices.max_cost(
            u64::try_from(input_bound).unwrap_or(u64::MAX),
            content.max_output_tokens,
        ) else {
            return draft.finish(Outcome::BudgetExhausted, start);
        };
        if request
            .spend_ceiling
            .is_some_and(|ceiling| max_cost > ceiling)
        {
            return draft.finish(Outcome::BudgetExhausted, start);
        }
        let reservation = Reservation {
            event_id: self.ids.next_id(),
            call_id: request.call_id.clone(),
            agent_id: request.agent_id.clone(),
            purpose: request.purpose,
            model: request.model.clone(),
            endpoint: endpoint.id.clone(),
            key_owner: request.key_owner,
            price_table_ref: endpoint.prices.version(),
            max_cost,
            risk_day: new_york_date_and_hour(start).map_or(start.date(), |(day, _)| day),
        };
        let reservation = match self.meter.reserve(reservation) {
            Ok(r) => r,
            Err(MeterRefusal::BudgetExhausted) => {
                return draft.finish(Outcome::BudgetExhausted, start);
            }
            Err(MeterRefusal::Orphaned) => {
                self.orphans = self.orphans.saturating_add(1);
                return draft.finish(Outcome::MeterUnavailable, start);
            }
            Err(MeterRefusal::Unavailable) => {
                return draft.finish(Outcome::MeterUnavailable, start);
            }
        };
        self.admitted.push((start, request.agent_id.clone()));
        draft.record.reservation = Some(reservation.event_id.clone());
        let record = self.attempts(draft, &entry, &endpoint, &http, max_cost);
        self.settle(&reservation, &record);
        if record.outcome == Outcome::Ok
            && record.completed_at <= record.deadline
            && let Some(response_ref) = record.response_ref
        {
            self.cache.insert(
                CacheKey {
                    workspace: self.settings.workspace_id.clone(),
                    content_hash: entry.content_hash(),
                    request_digest: digest,
                },
                Cached {
                    output: record.output.clone(),
                    tool_calls: record.tool_calls.clone(),
                    response_ref,
                    reported_identity: record.reported_identity.clone(),
                },
            );
        }
        record
    }

    /// The first healthy endpoint, in registration order, that the policy allows and whose host the
    /// egress allowlist permits.
    fn endpoint(&self, entry: &Entry, allowlist: &EgressAllowlist) -> Option<Endpoint> {
        entry
            .endpoints()
            .find(|(endpoint, healthy)| {
                *healthy
                    && self.settings.policy.allows(endpoint)
                    && allowlist.permits(&endpoint.host)
            })
            .map(|(endpoint, _)| endpoint.clone())
    }

    fn cache_hit(
        &self,
        draft: &mut Draft,
        entry: &Entry,
        digest: Digest,
        at: UtcNanos,
    ) -> Option<CallRecord> {
        let key = CacheKey {
            workspace: self.settings.workspace_id.clone(),
            content_hash: entry.content_hash(),
            request_digest: digest,
        };
        let cached = self.cache.get(&key)?;
        let mut record = draft.record.clone();
        record.cache_hit = true;
        record.output = cached.output.clone();
        record.tool_calls = cached.tool_calls.clone();
        record.response_ref = Some(cached.response_ref);
        record.reported_identity = cached.reported_identity.clone();
        record.outcome = Outcome::Ok;
        record.completed_at = at;
        Some(record)
    }

    fn within_rate(&mut self, agent: Option<&str>, now: UtcNanos) -> bool {
        let since = before_ms(now, RATE_WINDOW_MS);
        self.admitted.retain(|(at, _)| *at > since);
        let workspace = self.admitted.len();
        let mine = self
            .admitted
            .iter()
            .filter(|(_, a)| a.as_deref() == agent)
            .count();
        let fits = |count: usize, cap: u32| u32::try_from(count).is_ok_and(|n| n < cap);
        fits(workspace, self.settings.rate.per_workspace)
            && (agent.is_none() || fits(mine, self.settings.rate.per_agent))
    }

    fn attempts(
        &mut self,
        mut draft: Draft,
        entry: &Entry,
        endpoint: &Endpoint,
        http: &HttpRequest,
        max_cost: Usd6,
    ) -> CallRecord {
        let deadline = draft.record.deadline;
        let key = match endpoint.locality {
            Locality::Hosted => match self.secrets.provider_key(&endpoint.route.upstream) {
                Ok(key) => Some(key),
                Err(_) => {
                    let now = self.clock.now();
                    return draft.finish(Outcome::CredentialInvalid, now);
                }
            },
            Locality::Local => None,
        };
        let mut cost = Usd6::ZERO;
        let mut usage = Usage::default();
        let mut unknown_cost = false;
        let mut outcome = Outcome::DeadlineExceeded;
        for attempt in 1..=MAX_ATTEMPTS {
            if self.clock.now() >= deadline {
                break;
            }
            draft.record.attempts = attempt;
            let delivery = self
                .transport
                .send(http, key.as_ref(), deadline, &mut self.clock);
            let done = self.clock.now();
            let attempt = match delivery {
                Delivery::ConnectFailed => Attempt::Retry(Outcome::ProviderUnavailable),
                Delivery::TimedOut => {
                    unknown_cost = true;
                    Attempt::Failed(Outcome::DeadlineExceeded)
                }
                Delivery::Response {
                    status,
                    body,
                    retry_after_ms,
                } => {
                    let reply = self.adapter.reply(&body);
                    match &reply {
                        Some(r) if !readable(r.usage) => unknown_cost = true,
                        Some(r) => match endpoint.prices.cost(r.usage) {
                            Ok(c) => {
                                cost = cost.checked_add(c).unwrap_or(max_cost);
                                usage = add_usage(usage, r.usage);
                            }
                            Err(_) => unknown_cost = true,
                        },
                        None if status == 200 => unknown_cost = true,
                        None => {}
                    }
                    if done > deadline {
                        Attempt::Failed(Outcome::DeadlineExceeded)
                    } else {
                        self.classify(
                            &mut draft,
                            entry,
                            endpoint,
                            status,
                            body,
                            reply,
                            retry_after_ms,
                            done,
                        )
                    }
                }
            };
            match attempt {
                Attempt::Output {
                    output,
                    tool_calls,
                    reply_ref,
                    body,
                } => {
                    draft.record.output = output;
                    draft.record.tool_calls = tool_calls;
                    draft.record.response_ref = Some(reply_ref);
                    draft.record.response = Some(body);
                    outcome = Outcome::Ok;
                    break;
                }
                Attempt::Failed(kind) => {
                    outcome = kind;
                    break;
                }
                Attempt::Retry(kind) if billed(cost, unknown_cost) => {
                    outcome = kind;
                    break;
                }
                Attempt::Retry(kind) => outcome = kind,
            }
        }
        draft.record.tokens = usage;
        draft.record.cost = if unknown_cost {
            max_cost.max(cost)
        } else {
            cost
        };
        let now = self.clock.now();
        if outcome == Outcome::Ok && now > deadline {
            draft.record.response_ref = None;
            draft.record.response = None;
            return draft.finish(Outcome::DeadlineExceeded, now);
        }
        draft.finish(outcome, now)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one attempt's facts, read once; a struct would only rename them"
    )]
    fn classify(
        &mut self,
        draft: &mut Draft,
        entry: &Entry,
        endpoint: &Endpoint,
        status: u16,
        body: Vec<u8>,
        reply: Option<Reply>,
        retry_after_ms: Option<u64>,
        done: UtcNanos,
    ) -> Attempt {
        match status {
            200 => {}
            401 | 403 => return Attempt::Failed(Outcome::CredentialInvalid),
            429 => {
                let wait = retry_after_ms.unwrap_or(DEFAULT_RETRY_AFTER_MS);
                return match after_ms(done, wait) {
                    Some(resume) if resume < draft.record.deadline => {
                        self.clock.wait_until(resume);
                        Attempt::Retry(Outcome::RateLimitedProvider)
                    }
                    _ => Attempt::Failed(Outcome::RateLimitedProvider),
                };
            }
            500..=599 => return Attempt::Retry(Outcome::ProviderUnavailable),
            _ => return Attempt::Failed(Outcome::ProviderUnavailable),
        }
        let reply_ref = Digest::of(&body);
        draft.record.response_ref = Some(reply_ref);
        draft.record.response = Some(body.clone());
        let reported = self.adapter.reported(&body);
        draft.record.reported_identity = reported.text();
        if !entry.serves(endpoint, &reported) {
            let _ = self
                .registry
                .set_healthy(&draft.record.model, &endpoint.id, false);
            return Attempt::Failed(Outcome::IdentityMismatch);
        }
        let Some(reply) = reply else {
            return Attempt::Failed(Outcome::SchemaInvalid);
        };
        draft.record.provider_request_id = reply.request_id.clone();
        if reply.refused {
            return Attempt::Failed(Outcome::ContentRefused);
        }
        if !reply.tool_calls.is_empty() {
            let offered: Vec<&ToolDeclaration> = draft.plan.offered.iter().collect();
            return match tools::validate(&reply.tool_calls, &offered, draft.plan.per_turn) {
                Ok(tool_calls) => Attempt::Output {
                    output: None,
                    tool_calls,
                    reply_ref,
                    body,
                },
                Err(_) => Attempt::Failed(Outcome::SchemaInvalid),
            };
        }
        let output = reply
            .content
            .as_deref()
            .and_then(|text| parse(text.as_bytes()).ok())
            .filter(|value| schema::fits(&draft.plan.output_schema, value));
        match output {
            Some(output) => Attempt::Output {
                output: Some(output),
                tool_calls: Vec::new(),
                reply_ref,
                body,
            },
            None => Attempt::Failed(Outcome::SchemaInvalid),
        }
    }

    fn settle(&mut self, reservation: &Reservation, record: &CallRecord) {
        let settlement = Settlement {
            event_id: self.ids.next_id(),
            call_id: reservation.call_id.clone(),
            reservation: reservation.event_id.clone(),
            outcome: record.outcome,
            attempts: record.attempts,
            tokens: record.tokens,
            cost: record.cost,
        };
        let _ = self.meter.settle(settlement);
    }
}

/// The largest integer the journal records (journal spec §4.4); a reply reporting more tokens is
/// unreadable and settles at its reservation's maximum.
const MAX_TOKENS: u64 = (1 << 53) - 1;

fn readable(usage: Usage) -> bool {
    usage.input <= MAX_TOKENS && usage.output <= MAX_TOKENS && usage.cached <= MAX_TOKENS
}

/// An attempt the provider billed, or may have, is never retried: the reservation holds one
/// attempt's maximum, so a call is billed for at most one attempt and settles within it (INF-6).
fn billed(cost: Usd6, unknown_cost: bool) -> bool {
    unknown_cost || cost > Usd6::ZERO
}

/// A request the gateway cannot make at all; it is refused before anything is recorded, sent,
/// reserved, or charged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    #[error("the request names another workspace (INF-9)")]
    ForeignWorkspace,
    #[error("the compiler and an evaluation name no agent, and every other purpose names one")]
    AgentPurpose,
    #[error("the call id is not a ULID")]
    CallId,
    #[error("the registry holds no entry for this pin")]
    UnknownModel,
    #[error("the pin's content hash differs from the registered entry's")]
    HashMismatch,
    #[error("a run kind, a transcript, or offered tools for a single_call entry")]
    NotAgentic,
    #[error("an agentic entry's call names no run kind, or one the entry does not serve")]
    RunKind,
    #[error("an offered tool the entry does not declare for the run kind, or one offered twice")]
    Tools,
    #[error(
        "the transcript is not a run of the entry's validated tool calls, each with one result, within max_turns"
    )]
    Transcript,
    /// The caller's own: the exchange with the model-gateway service was lost before its whole
    /// reply arrived, by its deadline (DEC-2397 item 2). The gateway never returns it.
    #[error("the model-gateway service did not answer the call whole by its deadline")]
    GatewayUnreachable,
}

impl RequestError {
    /// Every refusal, in the order the gateway checks them, then the caller's own
    /// [`RequestError::GatewayUnreachable`], which the gateway never returns.
    pub const ALL: [Self; 10] = [
        Self::ForeignWorkspace,
        Self::AgentPurpose,
        Self::CallId,
        Self::UnknownModel,
        Self::HashMismatch,
        Self::NotAgentic,
        Self::RunKind,
        Self::Tools,
        Self::Transcript,
        Self::GatewayUnreachable,
    ];

    /// The refusal's code: its name on the wire and in a log line.
    pub fn code(self) -> &'static str {
        match self {
            Self::ForeignWorkspace => "foreign_workspace",
            Self::AgentPurpose => "agent_purpose",
            Self::CallId => "call_id",
            Self::UnknownModel => "unknown_model",
            Self::HashMismatch => "hash_mismatch",
            Self::NotAgentic => "not_agentic",
            Self::RunKind => "run_kind",
            Self::Tools => "tools",
            Self::Transcript => "transcript",
            Self::GatewayUnreachable => "gateway_unreachable",
        }
    }
}

fn add_usage(a: Usage, b: Usage) -> Usage {
    Usage {
        input: a.input.saturating_add(b.input),
        output: a.output.saturating_add(b.output),
        cached: a.cached.saturating_add(b.cached),
    }
}

/// A record with nothing sent: every refusal's shape (journal spec rules 73 and 146).
fn skeleton(request: &CallRequest, at: UtcNanos) -> CallRecord {
    CallRecord {
        call_id: request.call_id.clone(),
        agent_id: request.agent_id.clone(),
        purpose: request.purpose,
        model: request.model.clone(),
        endpoint: "none".to_owned(),
        request_digest: None,
        guard_rule: None,
        seed: None,
        prompt_ref: None,
        response_ref: None,
        reported_identity: None,
        provider_request_id: None,
        outcome: Outcome::PolicyDenied,
        attempts: 0,
        tokens: Usage::default(),
        cost: Usd6::ZERO,
        price_table_ref: Digest::ZERO,
        cache_hit: false,
        deadline: request.deadline,
        completed_at: at,
        reservation: None,
        observations: request.observations.clone(),
        output: None,
        tool_calls: Vec::new(),
        prompt: None,
        response: None,
    }
}

/// Opens and closes each input's data section; an input that contains either marker is refused,
/// so retrieved text cannot close its own section (inference spec §8.3).
pub const DATA_OPEN: &str = "<<<data>>>";
pub const DATA_CLOSE: &str = "<<<end data>>>";

/// The pinned user text with each slot filled, or `None` when the inputs differ from the
/// template's contract or an input holds a section marker.
pub fn render(template: &Template, inputs: &BTreeMap<String, String>) -> Option<Prompt> {
    let names: Vec<&str> = inputs.keys().map(String::as_str).collect();
    let listed: Vec<&str> = template.inputs.iter().map(String::as_str).collect();
    if names != listed {
        return None;
    }
    let mut user = template.user.clone();
    for (name, value) in inputs {
        if value.contains(DATA_OPEN) || value.contains(DATA_CLOSE) || value.contains("{{") {
            return None;
        }
        user = user.replace(
            &format!("{{{{{name}}}}}"),
            &format!("{DATA_OPEN}\n{value}\n{DATA_CLOSE}"),
        );
    }
    Some(Prompt {
        system: template.system.clone(),
        user,
        tools: Vec::new(),
        transcript: Vec::new(),
    })
}

/// The transcript with each result inside its own data section, or `None` when a result holds a
/// section marker or a slot, so a result cannot close its own section (inference spec §8.3).
pub fn spotlight(transcript: &[TranscriptTurn]) -> Option<Vec<TranscriptTurn>> {
    transcript
        .iter()
        .map(|turn| {
            let results = turn
                .results
                .iter()
                .map(|result| {
                    let marked = result.contains(DATA_OPEN)
                        || result.contains(DATA_CLOSE)
                        || result.contains("{{");
                    (!marked).then(|| format!("{DATA_OPEN}\n{result}\n{DATA_CLOSE}"))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(TranscriptTurn {
                calls: turn.calls.clone(),
                results,
            })
        })
        .collect()
}

/// The prompt's canonical object: its system and user text and, only when present, the offered
/// tools' declarations and the transcript, so a `single_call` prompt is what it was before
/// inference spec v0.4.
fn prompt_value(prompt: &Prompt) -> Value {
    let mut value = object([
        ("system", Value::Str(prompt.system.clone())),
        ("user", Value::Str(prompt.user.clone())),
    ]);
    if let Value::Object(members) = &mut value {
        if !prompt.tools.is_empty()
            && let Ok(key) = mandate_canon::Key::new("tools")
        {
            members.insert(
                key,
                Value::Array(
                    prompt
                        .tools
                        .iter()
                        .map(ToolDeclaration::canonical)
                        .collect(),
                ),
            );
        }
        if !prompt.transcript.is_empty()
            && let Ok(key) = mandate_canon::Key::new("transcript")
        {
            members.insert(
                key,
                Value::Array(
                    prompt
                        .transcript
                        .iter()
                        .map(TranscriptTurn::canonical)
                        .collect(),
                ),
            );
        }
    }
    value
}

/// The bytes of the rendered prompt as stored: canonical JSON of its parts. Its length bounds the
/// call's input tokens, the offered tools' declarations included (inference spec §7.2).
pub fn prompt_bytes(prompt: &Prompt) -> Vec<u8> {
    to_canonical(&prompt_value(prompt))
}

/// The canonical request's digest (inference spec §3.1) for a `single_call` entry.
pub fn request_digest(entry: &Entry, prompt: &Prompt, as_of: UtcNanos) -> Digest {
    request_digest_for(entry, prompt, as_of, None, &entry.content().output_schema)
}

/// The canonical request's digest (inference spec §3.1): the identity, the rendered prompt with its
/// offered tools and transcript, the sampling, the output schema the reply is held to,
/// `max_output_tokens`, `as_of`, and the run kind. The tools, the transcript, the run kind, and
/// the sampling's `top_p` and reasoning are left out when absent, so a `single_call` digest, and
/// every recorded fake keyed by one, is what it was before inference spec v0.4.
pub fn request_digest_for(
    entry: &Entry,
    prompt: &Prompt,
    as_of: UtcNanos,
    run_kind: Option<&str>,
    output_schema: &Value,
) -> Digest {
    let content = entry.content();
    let mut canonical = object([
        ("as_of", Value::Str(as_of.to_string())),
        ("identity", content.identity.canonical()),
        (
            "max_output_tokens",
            int(u64::from(content.max_output_tokens)),
        ),
        ("output_schema", output_schema.clone()),
        ("prompt", prompt_value(prompt)),
        ("sampling", content.sampling()),
    ]);
    if let (Some(kind), Value::Object(members), Ok(key)) = (
        run_kind,
        &mut canonical,
        mandate_canon::Key::new("run_kind"),
    ) {
        members.insert(key, Value::Str(kind.to_owned()));
    }
    Digest::of(&to_canonical(&canonical))
}
