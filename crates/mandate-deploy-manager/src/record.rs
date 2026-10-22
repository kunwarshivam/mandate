//! The manager's three records (journal spec §9.21), as canonical drafts for the control stream.

use mandate_canon::{Digest, Int, Key, Value, encode_ulid, to_canonical};
use mandate_process::CRASH_LOOP_WINDOW;
use mandate_time::UtcNanos;

use crate::control::{Cause, Fact, Slot, StopCause};
use crate::orchestrator::{CRASH_LOOP, Effect};

/// Who writes the records: the workspace, and the manager as a `system` actor with its build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub workspace: String,
    pub actor_id: String,
    pub version: String,
    pub build: Digest,
}

impl Identity {
    /// The workspace control stream, `ctl:{workspace_id}` (journal spec §2).
    pub fn stream(&self) -> String {
        format!("ctl:{}", self.workspace)
    }
}

/// One record the manager journals before its effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// `ProcessStartIssued`; a kill switch's start names the switch as its cause.
    Start {
        slot: Slot,
        generation: u64,
        cause: Cause,
        kill_switch: Option<String>,
    },
    /// `ProcessStopIssued`: for an agent stopped, a token exchange whose request closed, or a
    /// connect's executor whose connection closed.
    Stop {
        slot: Slot,
        generation: u64,
        cause: StopCause,
    },
    /// `ProcessCrashLooped`: the pause.
    CrashLooped {
        slot: Slot,
        generation: u64,
        restarts: u64,
    },
}

impl Record {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Start { .. } => "ProcessStartIssued",
            Self::Stop { .. } => "ProcessStopIssued",
            Self::CrashLooped { .. } => "ProcessCrashLooped",
        }
    }

    pub fn slot(&self) -> &Slot {
        match self {
            Self::Start { slot, .. } | Self::Stop { slot, .. } | Self::CrashLooped { slot, .. } => {
                slot
            }
        }
    }

    pub fn generation(&self) -> u64 {
        match self {
            Self::Start { generation, .. }
            | Self::Stop { generation, .. }
            | Self::CrashLooped { generation, .. } => *generation,
        }
    }

    /// What the record allows once it is committed.
    pub fn effect(&self) -> Effect {
        let slot = self.slot().clone();
        let generation = self.generation();
        match self {
            Self::Start { .. } => Effect::Start { slot, generation },
            Self::Stop { .. } => Effect::Stop { slot, generation },
            Self::CrashLooped { .. } => Effect::Alert(CRASH_LOOP),
        }
    }

    /// The fact the committed record is read back as, recorded at `at`.
    pub fn fact(&self, at: UtcNanos) -> Fact {
        let slot = self.slot().clone();
        let generation = self.generation();
        match self {
            Self::Start { cause, .. } => Fact::Started {
                slot,
                generation,
                cause: *cause,
                at,
            },
            Self::Stop { .. } => Fact::StopIssued { slot, generation },
            Self::CrashLooped { .. } => Fact::CrashLooped { slot },
        }
    }

    /// The record's `event_id`: a ULID derived from the stream, the record, the slot and the
    /// generation, so a retried or a second manager's draft of the same step is the same event and
    /// the journal commits it once (journal spec §5.1).
    pub fn event_id(&self, identity: &Identity) -> String {
        let stream = identity.stream();
        let generation = self.generation().to_string();
        let digest = Digest::of_parts(&[
            stream.as_bytes(),
            self.event_type().as_bytes(),
            self.slot().process.code().as_bytes(),
            self.slot().subject.as_bytes(),
            generation.as_bytes(),
        ]);
        let mut high = [0_u8; 16];
        for (to, from) in high.iter_mut().zip(digest.as_bytes()) {
            *to = *from;
        }
        encode_ulid(u128::from_be_bytes(high))
    }

    /// The canonical draft for `identity`'s control stream in `environment`, at `now`.
    pub fn draft(&self, identity: &Identity, environment: &str, now: UtcNanos) -> Vec<u8> {
        let slot = self.slot();
        let mut payload = vec![
            ("process", text(slot.process.code())),
            ("subject", text(&slot.subject)),
            ("generation", int(self.generation())),
        ];
        let mut causation = Value::Null;
        match self {
            Self::Start {
                cause, kill_switch, ..
            } => {
                payload.push(("cause", text(cause.code())));
                if let Some(id) = kill_switch {
                    causation = text(id);
                }
            }
            Self::Stop { cause, .. } => payload.push(("cause", text(cause.code()))),
            Self::CrashLooped { restarts, .. } => {
                payload.push(("restarts", int(*restarts)));
                payload.push(("window_seconds", int(CRASH_LOOP_WINDOW.as_secs())));
            }
        }
        let actor = object(vec![
            ("kind", text("system")),
            ("id", text(&identity.actor_id)),
            ("version", text(&identity.version)),
            (
                "build",
                text(&format!("sha256:{}", identity.build.to_hex())),
            ),
        ]);
        to_canonical(&object(vec![
            ("envelope_version", int(1)),
            ("environment", text(environment)),
            ("event_id", text(&self.event_id(identity))),
            ("stream_id", text(&identity.stream())),
            ("event_type", text(self.event_type())),
            ("schema_version", int(1)),
            ("event_time", text(&now.to_string())),
            ("clock_source", text("local")),
            ("causation_id", causation),
            ("correlation_id", Value::Null),
            ("actor", actor),
            ("config_refs", object(Vec::new())),
            ("payload", object(payload)),
            ("artifact_refs", Value::Array(Vec::new())),
            ("pii_refs", Value::Array(Vec::new())),
        ]))
    }
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// An integer member; one the canonical form cannot hold becomes `null`, which the journal
/// refuses, so the record is never committed with another number.
fn int(value: u64) -> Value {
    Int::new(value).map_or(Value::Null, Value::Int)
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .filter_map(|(key, value)| Some((Key::new(key).ok()?, value)))
            .collect(),
    )
}
