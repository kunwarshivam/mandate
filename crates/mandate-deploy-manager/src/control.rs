//! What the manager reads from the workspace control stream: the deployments, the connections
//! they run on, the stops, the kill switches, and its own records (journal spec §9.2, §9.21,
//! §9.24). Every fact is read from the journal alone: no store outside it stands between a
//! deployment, or a kill switch, and its processes (DEC-1171 item 4).

use mandate_canon::{Value, parse};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

/// The two processes of an agent deployment the manager runs (infrastructure design §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Process {
    /// The agent's runtime, its agent stream's single writer.
    Runtime,
    /// The account's executor, its account stream's single writer.
    Executor,
}

impl Process {
    pub fn code(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Executor => "executor",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "runtime" => Some(Self::Runtime),
            "executor" => Some(Self::Executor),
            _ => None,
        }
    }
}

/// One process the manager may run: an agent's runtime, or an account's executor. Each writes one
/// stream, so a slot has at most one live instance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot {
    pub process: Process,
    /// The agent, for a runtime; the `account_ref`, for an executor.
    pub subject: String,
}

impl Slot {
    pub fn runtime(agent: &str) -> Self {
        Self {
            process: Process::Runtime,
            subject: agent.to_owned(),
        }
    }

    pub fn executor(account: &str) -> Self {
        Self {
            process: Process::Executor,
            subject: account.to_owned(),
        }
    }
}

/// What a kill switch covers (trading spec §5.5). A connection covers its account's executor and
/// that account's agents' runtimes, its account read from the control stream's own
/// `ConnectionEstablished`, so no store outside the journal stands between a switch and its scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Agent(String),
    Connection(String),
    Workspace,
}

/// Why a start was journaled (journal spec §9.21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Deployed,
    Restart,
    KillSwitch,
}

impl Cause {
    pub fn code(self) -> &'static str {
        match self {
            Self::Deployed => "deployed",
            Self::Restart => "restart",
            Self::KillSwitch => "kill_switch",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        match code {
            "deployed" => Some(Self::Deployed),
            "restart" => Some(Self::Restart),
            "kill_switch" => Some(Self::KillSwitch),
            _ => None,
        }
    }
}

/// One control-stream event, as the manager reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    /// The stream's `StreamOpened`, whose environment every record of the manager repeats.
    Opened {
        environment: String,
    },
    /// A connection's account, from its `ConnectionEstablished` (journal spec §9.8); version 1,
    /// which names no account, is not read.
    Connected {
        connection: String,
        account: String,
    },
    /// A connection's `ConnectionRevoked`: a deployment on it after this starts nothing until it is
    /// established again (rule 201); one deployed before keeps running (§9.24).
    Revoked {
        connection: String,
    },
    /// A version-2 `AgentDeployed`, which names its connection (§9.24).
    Deployed {
        agent: String,
        connection: String,
    },
    /// A version-1 `AgentDeployed`, which names no connection, so no account: nothing starts for it
    /// (DEC-1171 item 4), and the manager reports it (DEC-1173 item 3).
    DeployedUnread {
        agent: String,
    },
    Stopped {
        agent: String,
    },
    KillSwitch {
        event_id: String,
        scope: Scope,
    },
    Started {
        slot: Slot,
        generation: u64,
        cause: Cause,
        at: UtcNanos,
    },
    StopIssued {
        slot: Slot,
        generation: u64,
    },
    CrashLooped {
        slot: Slot,
    },
}

/// The fact `row` records, or `None` for a row the manager does not act on.
pub fn fact(row: &StoredEvent) -> Option<Fact> {
    let body = parse(&row.body).ok()?;
    let payload = body.get("payload")?;
    let text = |member: &str| payload.get(member).and_then(Value::as_str);
    let int = |member: &str| payload.get(member).and_then(Value::as_int);
    let slot = || {
        Some(Slot {
            process: Process::parse(text("process")?)?,
            subject: text("subject")?.to_owned(),
        })
    };
    match row.event_type.as_str() {
        "StreamOpened" => Some(Fact::Opened {
            environment: row.environment.clone(),
        }),
        "ConnectionEstablished" => Some(Fact::Connected {
            connection: text("connection_id")?.to_owned(),
            account: text("account_ref")?.to_owned(),
        }),
        "ConnectionRevoked" => Some(Fact::Revoked {
            connection: text("connection_id")?.to_owned(),
        }),
        "AgentDeployed" if row.schema_version == 2 => Some(Fact::Deployed {
            agent: text("agent_id")?.to_owned(),
            connection: text("connection_id")?.to_owned(),
        }),
        "AgentDeployed" => Some(Fact::DeployedUnread {
            agent: text("agent_id")?.to_owned(),
        }),
        "AgentStopped" => Some(Fact::Stopped {
            agent: text("agent_id")?.to_owned(),
        }),
        "OwnerCommandIssued" if text("command") == Some("kill_switch") => {
            let subject = text("subject")?;
            let scope = match text("scope")? {
                "agent" => Scope::Agent(subject.to_owned()),
                "connection" => Scope::Connection(subject.to_owned()),
                "workspace" => Scope::Workspace,
                _ => return None,
            };
            Some(Fact::KillSwitch {
                event_id: row.event_id.clone(),
                scope,
            })
        }
        "ProcessStartIssued" => Some(Fact::Started {
            slot: slot()?,
            generation: int("generation")?,
            cause: Cause::parse(text("cause")?)?,
            at: UtcNanos::parse(&row.recorded_at).ok()?,
        }),
        "ProcessStopIssued" => Some(Fact::StopIssued {
            slot: slot()?,
            generation: int("generation")?,
        }),
        "ProcessCrashLooped" => Some(Fact::CrashLooped { slot: slot()? }),
        _ => None,
    }
}
