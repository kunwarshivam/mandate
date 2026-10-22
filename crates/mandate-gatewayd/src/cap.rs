//! Each agent's daily research cap, read by the service from the deployed mandate (DEC-2272,
//! DEC-2254, E21-62): the agent's deployed version from its workspace's control stream, the
//! version's artifact checked against its hash, and `behavior.research.cost_cap_usd_per_day`
//! tightened by the internal research profile. Nothing the caller sends sets it, and anything that
//! does not read refuses with its code rather than leaving a cap the confirmed mandate did not give.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};
use mandate_gateway::money::Usd6;
use mandate_journal::{ArtifactRef, ArtifactSource, Environment, StoredEvent, get_artifact};
use mandate_num::Usd;
use mandate_research::PolicyOverlay;

/// Why an agent's cap could not be read. Each names only its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapError {
    #[error("the control stream does not read")]
    ControlUnreadable,
    #[error("the deployed mandate's document gives no cap the gateway can hold")]
    DocumentUnavailable,
    #[error("the deployed mandate is for another environment")]
    EnvironmentMismatch,
}

impl CapError {
    /// The code the start exits with, or a call's refusal is logged under.
    pub fn code(self) -> &'static str {
        match self {
            Self::ControlUnreadable => "control_unreadable",
            Self::DocumentUnavailable => "mandate_document_unavailable",
            Self::EnvironmentMismatch => "mandate_environment_mismatch",
        }
    }
}

/// What an agent's latest deployment record on the control stream says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deployment {
    /// Deployed with this mandate version, an artifact reference.
    Deployed(String),
    /// Never deployed in this environment, or stopped since.
    NotDeployed,
}

/// The agent's latest `AgentDeployed` or `AgentStopped` in `environment` (DEC-2272 item 1).
///
/// # Errors
/// [`CapError::ControlUnreadable`] for a deployment record whose body does not read.
pub fn deployment(
    control: &[StoredEvent],
    agent: &str,
    environment: Environment,
) -> Result<Deployment, CapError> {
    for row in control.iter().rev().filter(|row| {
        row.environment == environment.as_str()
            && matches!(row.event_type.as_str(), "AgentDeployed" | "AgentStopped")
    }) {
        let body = parse(&row.body).map_err(|_| CapError::ControlUnreadable)?;
        let payload = body.get("payload").ok_or(CapError::ControlUnreadable)?;
        let named = text(payload, "agent_id").ok_or(CapError::ControlUnreadable)?;
        if named != agent {
            continue;
        }
        if row.event_type == "AgentStopped" {
            return Ok(Deployment::NotDeployed);
        }
        let version = text(payload, "mandate_version").ok_or(CapError::ControlUnreadable)?;
        return Ok(Deployment::Deployed(version.to_owned()));
    }
    Ok(Deployment::NotDeployed)
}

/// Every agent an `AgentDeployed` in `environment` names, deployed now or stopped since: the agents
/// whose caps the start reads (DEC-2272 item 4).
///
/// # Errors
/// [`CapError::ControlUnreadable`] for an `AgentDeployed` whose body does not read.
pub fn agents(
    control: &[StoredEvent],
    environment: Environment,
) -> Result<BTreeSet<String>, CapError> {
    control
        .iter()
        .filter(|row| row.environment == environment.as_str() && row.event_type == "AgentDeployed")
        .map(|row| {
            let body = parse(&row.body).map_err(|_| CapError::ControlUnreadable)?;
            body.get("payload")
                .and_then(|payload| text(payload, "agent_id"))
                .map(str::to_owned)
                .ok_or(CapError::ControlUnreadable)
        })
        .collect()
}

/// The cap `deployment` gives (DEC-2272 items 1 to 4): zero when not deployed or when the
/// document's `behavior.research` is `null`.
///
/// # Errors
/// [`CapError::DocumentUnavailable`] for an artifact that is missing, does not hash to its
/// reference or does not parse, a cap missing, malformed, beyond `Usd6` or above
/// `workspace_monthly`; [`CapError::EnvironmentMismatch`] for another environment's document.
pub fn research_cap(
    deployment: &Deployment,
    environment: Environment,
    store: &dyn ArtifactSource,
    workspace_monthly: Usd6,
) -> Result<Usd6, CapError> {
    let Deployment::Deployed(version) = deployment else {
        return Ok(Usd6::ZERO);
    };
    let unavailable = CapError::DocumentUnavailable;
    let document = ArtifactRef::parse(version)
        .and_then(|reference| get_artifact(store, &reference).ok())
        .and_then(|bytes| parse(&bytes).ok())
        .ok_or(unavailable)?;
    if text(&document, "environment") != Some(environment.as_str()) {
        return Err(CapError::EnvironmentMismatch);
    }
    let research = document
        .get("behavior")
        .and_then(|behavior| behavior.get("research"))
        .ok_or(unavailable)?;
    if *research == Value::Null {
        return Ok(Usd6::ZERO);
    }
    let cap = text(research, "cost_cap_usd_per_day")
        .and_then(|text| Usd::parse(text).ok())
        .ok_or(unavailable)?;
    let cap = PolicyOverlay::internal_research_profile().effective_cost_cap(cap);
    Usd6::parse(&cap.to_string())
        .ok()
        .filter(|cap| *cap <= workspace_monthly)
        .ok_or(unavailable)
}

/// Each agent's cap with the deployment it was read from, read again whenever the control stream
/// names another (DEC-2272 item 5): a call is only ever admitted against the cap of the version
/// deployed now.
pub struct DailyCaps {
    environment: Environment,
    workspace_monthly: Usd6,
    held: BTreeMap<String, (Deployment, Usd6)>,
}

impl DailyCaps {
    pub fn new(environment: Environment, workspace_monthly: Usd6) -> Self {
        Self {
            environment,
            workspace_monthly,
            held: BTreeMap::new(),
        }
    }

    /// How many agents' caps are held: only deployed agents' (DEC-2272 item 3), so the count grows
    /// with nothing a caller sends.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn held(&self) -> usize {
        let _ = &self.held;
        todo!()
    }

    /// The environment whose deployments the caps are read for.
    pub fn environment(&self) -> Environment {
        self.environment
    }

    /// `agent`'s cap for the deployment `control` names now: the one held when it is unchanged,
    /// else read again. A read that fails drops what was held, so the agent has no cap to use
    /// until a read succeeds.
    ///
    /// # Errors
    /// As [`deployment`] and [`research_cap`].
    pub fn current(
        &mut self,
        control: &[StoredEvent],
        agent: &str,
        store: &dyn ArtifactSource,
    ) -> Result<Usd6, CapError> {
        let read = deployment(control, agent, self.environment).and_then(|now| {
            match self.held.get(agent) {
                Some((held, cap)) if *held == now => Ok(*cap),
                _ => research_cap(&now, self.environment, store, self.workspace_monthly).inspect(
                    |&cap| {
                        self.held.insert(agent.to_owned(), (now, cap));
                    },
                ),
            }
        });
        if read.is_err() {
            self.held.remove(agent);
        }
        read
    }
}

fn text<'a>(value: &'a Value, member: &str) -> Option<&'a str> {
    value.get(member).and_then(Value::as_str)
}
