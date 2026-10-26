//! The door's run-level probes (E7-26, [DEC-918 item 3](../../docs/project/decisions/DEC-918.md)'s
//! call-site order, landed under the DEC-77 amendment's primary path: a review ruling required
//! the four probes, and the 2026-09-27 amendment puts ruling-required tests in-module, under
//! `#[cfg(test)]` in the crate's `src/`). `run()` is driven against recorded records and a stored
//! document alone — no network, no credential, no journal service (ES-19, `AGENTS.md` rules 7 and
//! 8) — and each probe pins one refusal the door owes ahead of the shell's own fold and of every
//! credential read (FT-3), keeping the planted-regression kills the row's review hunted: the
//! door behind the fold, after the credential read, gated by the placing branch or never called
//! fails these names exactly as the wiring's review of record requires.
//!
//! The fixtures follow `mandate-shell`'s own in-module precedent
//! (`crates/mandate-shell/src/paper/tests.rs`): the shared mandate fixture's bytes are read
//! from the suite's fixture file — the same file `tests/common/mod.rs`'s `spy_mandate`
//! derives its document from (after its AAPL-to-SPY and model-hash substitutions, which the
//! probes do not need: the door reads the deployed document's own members alone) — so no copy
//! of it can drift, and nothing else is imported from any tests directory.

use std::cell::Cell;
use std::fs;

use mandate_alpaca::AlpacaPaperHttp;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Value, to_canonical};
use mandate_domain::Environment;
use mandate_journal::ArtifactStore;
use mandate_shell::control::{ControlRecord, DeploymentRefusal, Origin};
use mandate_shell::{Cause, ShellError, Stage};

use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{Feed, LatestQuote};
use mandate_domain::AssetClass;
use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_robinhood::Quote;

use crate::{
    Args, BrokerAsk, CollarMembers, Connection, ConnectionRefusal, Dial, Opening, PaperError,
    Ports, QuoteRefusal, SystemClock, run,
};

/// The paper mandate fixture the suite derives its deployed document from
/// (`tests/common/mod.rs`'s `spy_mandate` over this file's bytes, after its AAPL-to-SPY and
/// model-hash substitutions): its connection `conn_alpaca_paper_01` is the connection the
/// records below establish and revoke, and its environment is the paper one a paper run keeps
/// (V-001, LT-14).
const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");

/// The run's account ref, the stream every record of the connection must bind: an opaque id
/// (TI-8).
const ACCOUNT: &str = "acct_probe";

/// The run's workspace, when the probe's form is one journal spec §2 holds.
const WORKSPACE: &str = "ws_probe";

/// A probe's ports: the recorded control stream, the system clock, and a connection counter that
/// proves no credential was read before the run's refusal (FT-3) — the transport type is the
/// binary's own, but `connect` answers only a count the probes assert is zero.
struct Probe {
    workspace: &'static str,
    records: Vec<ControlRecord>,
    connects: Cell<usize>,
    hardens: Cell<usize>,
}

impl Ports for Probe {
    type Session = AlpacaPaperHttp;
    type Pause = SystemClock;

    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError> {
        (workspace == self.workspace)
            .then(|| self.records.clone())
            .ok_or(PaperError::Control)
    }

    fn connect(
        &mut self,
        _connection: &Connection,
        _dial: Option<&Dial>,
    ) -> Result<AlpacaPaperHttp, PaperError> {
        self.connects.set(self.connects.get().saturating_add(1));
        Err(PaperError::Credentials)
    }

    fn harden(&mut self) -> Result<(), PaperError> {
        self.hardens.set(self.hardens.get().saturating_add(1));
        Ok(())
    }

    fn pause(&self) -> SystemClock {
        SystemClock
    }
}

/// One control-stream record at `seq`: a payload the canonical parser reads, or the parse's own
/// report.
fn record(seq: u64, event_type: &str, payload: &str) -> Result<ControlRecord, String> {
    Ok(ControlRecord {
        seq,
        event_time: None,
        event_type: event_type.to_owned(),
        payload: parse(payload)?,
        origin: Origin {
            stream_id: "ctl:ws_paper".to_owned(),
            environment: Environment::Paper,
        },
    })
}

/// `text`'s canonical value, or the parse's own report.
fn parse(text: &str) -> Result<Value, String> {
    mandate_canon::parse(text.as_bytes()).map_err(|refused| refused.to_string())
}

/// One probe's scene: the run's arguments over `workspace`, a store holding the deployed
/// `document` alone (every probe refuses at the door, before the shell's fold reads any
/// registration), and the control-stream records the probe hands — the agent's deployment of
/// that document, then `extra`, in journal order.
fn scene(
    name: &str,
    document: &str,
    workspace: &'static str,
    extra: Vec<ControlRecord>,
) -> Result<(Args, Probe), String> {
    let root = std::env::temp_dir().join(format!("g1a-door-probe-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|refused| refused.to_string())?;
    }
    let mut store =
        FsArtifactStore::open(&root).map_err(|_| "the store could not be opened".to_owned())?;
    let bytes = to_canonical(&parse(document)?);
    let version = store
        .put_artifact(&bytes)
        .map_err(|refused| refused.to_string())?;
    let deployed = format!(
        r#"{{"agent_id":"agent_spy","mandate_version":"{version}","record_ref":"sha256:{}"}}"#,
        "6".repeat(64)
    );
    let mut records = vec![record(1, "AgentDeployed", &deployed)?];
    records.extend(extra);
    let args = Args {
        workspace: workspace.to_owned(),
        agent: "agent_spy".to_owned(),
        account_ref: ACCOUNT.to_owned(),
        journal: None,
        store: root,
        bars: std::env::temp_dir()
            .join(format!("g1a-door-probe-{name}-bars-{}", std::process::id())),
        place_one_order: false,
        reconcile_only: false,
        acknowledged: Environment::Paper,
    };
    Ok((
        args,
        Probe {
            workspace,
            records,
            connects: Cell::new(0),
            hardens: Cell::new(0),
        },
    ))
}

/// DEC-910 item 2 and DEC-918 item 3, at `run()`'s own level: a run whose connection record is
/// revoked refuses with the door's exact message — `connection_revoked`, the refusal the shell's
/// fold would answer behind it with a V-001 violation — ahead of the fold and of every
/// credential read (FT-3: the counter the probe's ports hold reads zero).
#[test]
fn a_revoked_record_refuses_the_run_with_the_doors_exact_message() -> Result<(), String> {
    let extra = vec![
        record(
            2,
            "ConnectionEstablished",
            &format!(
                r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper"}}"#
            ),
        )?,
        record(
            3,
            "ConnectionRevoked",
            r#"{"connection_id":"conn_alpaca_paper_01"}"#,
        )?,
    ];
    let (args, mut probe) = scene("revoked", MANDATE, WORKSPACE, extra)?;
    let outcome = run(&args, &[], &mut probe);
    let refused = outcome
        .err()
        .ok_or_else(|| "the revocation must refuse the run".to_owned())?;
    assert!(
        matches!(refused, PaperError::Connection(ConnectionRefusal::Revoked)),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "the connection the mandate names was revoked"
    );
    assert_eq!(
        probe.connects.get(),
        0,
        "no credential read before the door"
    );
    Ok(())
}

/// DEC-918 item 3's central case, at `run()`'s own level: a live deployment answers through the
/// door — `connection_unrecorded`, the exact refusal the founder's run's step 4 asks for, naming
/// the record the mandate's connection lacks — ahead of the shell's own
/// `DeploymentRefusal::Live`, which stays for the shell's fold behind the door.
#[test]
fn a_live_deployment_refuses_through_the_door_ahead_of_the_shells_live() -> Result<(), String> {
    let live = MANDATE.replace(r#""paper""#, r#""live""#);
    let (args, mut probe) = scene("live-deployment", &live, WORKSPACE, vec![])?;
    let outcome = run(&args, &[], &mut probe);
    let refused = outcome
        .err()
        .ok_or_else(|| "the live deployment must refuse the run".to_owned())?;
    assert!(
        matches!(
            refused,
            PaperError::Connection(ConnectionRefusal::Unrecorded)
        ),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "the mandate's connection has no record on the control stream"
    );
    assert_eq!(
        probe.connects.get(),
        0,
        "no credential read before the door"
    );
    Ok(())
}

/// DEC-918 item 3's own bound at `run()`'s level — "the door performs no validation of its own,
/// **never answers where the shell refuses**, and only ever adds a refusal in front of the
/// shell's": a stopped agent is the shell's own answer, so the door's pre-read falls through it
/// and the fold answers `stopped` unchanged — even where the stopped deployment names a
/// **live-environment** document, the one walk where a wrongly-engaging door would sit in front
/// with its own refusal instead. Pinning the walk's `AgentStopped` arm exactly (the fold's own
/// stopped case, and zero connects: the fold refuses before any credential too).
#[test]
fn a_stopped_deployment_answers_at_the_fold_not_the_door() -> Result<(), String> {
    let live = MANDATE.replace(r#""paper""#, r#""live""#);
    let stopped = record(2, "AgentStopped", r#"{"agent_id":"agent_spy"}"#)?;
    let (args, mut probe) = scene("stopped-live", &live, WORKSPACE, vec![stopped])?;
    let outcome = run(&args, &[], &mut probe);
    let refused = outcome
        .err()
        .ok_or_else(|| "the stop must refuse the run".to_owned())?;
    assert!(
        matches!(refused, PaperError::Deployment(DeploymentRefusal::Stopped)),
        "{refused:?}: the fold's stopped answer, never the door's own"
    );
    assert_eq!(
        probe.connects.get(),
        0,
        "no credential read before the fold's own answer"
    );
    Ok(())
}

/// DEC-910 item 2's `connection_account`, at `run()`'s own level: the establishment of the
/// mandate's own connection binding another account stream refuses the run before anything
/// runs, with the door's exact message.
#[test]
fn a_record_binding_another_account_stream_refuses_the_run() -> Result<(), String> {
    let extra = vec![record(
        2,
        "ConnectionEstablished",
        r#"{"account_ref":"acct_other","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper"}"#,
    )?];
    let (args, mut probe) = scene("foreign-account", MANDATE, WORKSPACE, extra)?;
    let outcome = run(&args, &[], &mut probe);
    let refused = outcome
        .err()
        .ok_or_else(|| "the foreign account must refuse the run".to_owned())?;
    assert!(
        matches!(refused, PaperError::Connection(ConnectionRefusal::Account)),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "the record binds another account stream than the run's"
    );
    assert_eq!(
        probe.connects.get(),
        0,
        "no credential read before the door"
    );
    Ok(())
}

/// DEC-918 item 2 and journal spec §2, at `run()`'s own level: a dry run — no
/// `--place-one-order` — still names the account stream from its own arguments, ungated by the
/// placing flag, and a workspace whose form §2 can no stream hold fails closed with the absent
/// stream the connection binds at the reconcile stage, never a fallback onto another stream.
#[test]
fn an_unformable_workspace_refuses_a_dry_run_before_anything_runs() -> Result<(), String> {
    let (args, mut probe) = scene("naming", MANDATE, "ws:probe", vec![])?;
    let outcome = run(&args, &[], &mut probe);
    let refused = outcome
        .err()
        .ok_or_else(|| "the naming must refuse the run".to_owned())?;
    assert!(
        matches!(
            refused,
            PaperError::Shell(ShellError::Refused {
                stage: Stage::Reconcile,
                cause: Cause::Absent {
                    what: "the account stream the connection binds"
                }
            })
        ),
        "{refused:?}"
    );
    assert_eq!(
        probe.connects.get(),
        0,
        "no credential read before the door"
    );
    Ok(())
}

/// A clock whose every read is a nanosecond after the last, from `T0`.
struct Ticking(Cell<u32>);

impl mandate_alpaca::Pause for Ticking {
    fn now(&self) -> mandate_time::UtcNanos {
        let nanos = self.0.get();
        self.0.set(nanos.saturating_add(1));
        mandate_time::UtcNanos::from_parts(1_792_162_800, nanos)
            .unwrap_or(mandate_time::UtcNanos::EPOCH)
    }

    async fn pause(&self, _duration: std::time::Duration) {}
}

/// DEC-940 item 4 and DEC-945 item 3: the asked opening's cross-check is the collar's own over
/// the broker's ask, read at the check for the preflight IEX quote's instrument and held to the
/// opening's own quote age bound, and the IEX ask — agreeing asks pass, asks beyond the collar
/// refuse, a session with no broker ask refuses as a missing quote, a quote of another
/// instrument is missing too, and a read older than the bound is stale, so nothing leaves on a
/// check that was not made (rule 3).
#[test]
fn the_asked_openings_cross_check_reads_both_asks_within_the_collar() -> Result<(), String> {
    let parse = |text: &str| Price::parse(text).map_err(|error| error.to_string());
    let collar = CollarMembers {
        liquid_x: Fraction::parse("0.01").map_err(|error| error.to_string())?,
        other_x: Fraction::parse("0.02").map_err(|error| error.to_string())?,
        crypto_x: Fraction::parse("0.02").map_err(|error| error.to_string())?,
        liquid_threshold_usd: Usd::parse("50000000").map_err(|error| error.to_string())?,
    };
    let spy = InstrumentId::new("SPY").map_err(|error| format!("{error:?}"))?;
    let independent = LatestQuote {
        instrument: spy.clone(),
        at: mandate_time::UtcNanos::EPOCH,
        bid: parse("99")?,
        bid_size: Qty::parse("1").map_err(|error| error.to_string())?,
        ask: parse("100")?,
        ask_size: Qty::parse("1").map_err(|error| error.to_string())?,
        feed: Feed::Iex,
        clock_skew: None,
    };
    let opening = |broker_ask: BrokerAsk, quote_max_age: Duration| -> Result<Opening, String> {
        Ok(Opening {
            asset_class: AssetClass::UsEquity,
            median_dollar_volume_20d: Usd::parse("60000000").map_err(|error| error.to_string())?,
            broker_ask,
            independent: independent.clone(),
            quote_max_age,
        })
    };
    let read = |symbol: &str, ask: &str| -> Result<BrokerAsk, String> {
        let quote = Quote {
            symbol: InstrumentId::new(symbol).map_err(|error| format!("{error:?}"))?,
            bid: parse("1")?,
            ask: parse(ask)?,
        };
        Ok(BrokerAsk::Read(Box::new(move |asked: &InstrumentId| {
            (asked == &quote.symbol).then(|| quote.clone())
        })))
    };
    let clock = Ticking(Cell::new(0));
    let second = Duration::from_secs(1);
    assert!(matches!(
        opening(BrokerAsk::Quoted(parse("100.5")?), second)?.cross_check(&collar, &clock),
        Ok(())
    ));
    assert!(matches!(
        opening(BrokerAsk::Quoted(parse("101.5")?), second)?.cross_check(&collar, &clock),
        Err(PaperError::CrossCheck(QuoteRefusal::Disagreement))
    ));
    assert!(matches!(
        opening(BrokerAsk::Absent, second)?.cross_check(&collar, &clock),
        Err(PaperError::CrossCheck(QuoteRefusal::Absent))
    ));
    assert!(matches!(
        opening(read("SPY", "100.5")?, second)?.cross_check(&collar, &clock),
        Ok(())
    ));
    assert!(matches!(
        opening(read("SPY", "101.5")?, second)?.cross_check(&collar, &clock),
        Err(PaperError::CrossCheck(QuoteRefusal::Disagreement))
    ));
    assert!(matches!(
        opening(read("QQQ", "100")?, second)?.cross_check(&collar, &clock),
        Err(PaperError::CrossCheck(QuoteRefusal::Absent))
    ));
    assert!(matches!(
        opening(read("SPY", "100")?, Duration::ZERO)?.cross_check(&collar, &clock),
        Err(PaperError::CrossCheck(QuoteRefusal::Stale))
    ));
    Ok(())
}

/// One stored control row on `stream_id`, its envelope naming `environment`, with an empty
/// payload: the members `control_record` reads.
fn stored_row(stream_id: &str, environment: &str) -> mandate_journal::StoredEvent {
    mandate_journal::StoredEvent {
        stream_id: stream_id.to_owned(),
        seq: 3,
        event_id: format!("{:0>26}", 3),
        event_type: "ConfigSnapshotRegistered".to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: "2026-10-10T15:05:00Z".to_owned(),
        prev_hash: mandate_canon::Digest::of(&[]),
        hash: mandate_canon::Digest::of(&[]),
        body: br#"{"payload":{}}"#.to_vec(),
    }
}

/// DEC-957 item 4: the run's control read carries each row's own stream and environment into its
/// record, so the artifacts can bind only their own stream's registrations, and a row naming
/// neither environment refuses the read rather than being read as one.
#[test]
fn a_control_record_carries_its_rows_stream_and_environment() {
    for (environment, expected) in [("paper", Environment::Paper), ("live", Environment::Live)] {
        let record = crate::control_record(&stored_row("ctl:ws_live", environment));
        let origin = record.map(|record| record.origin);
        assert_eq!(
            origin.ok(),
            Some(Origin {
                stream_id: "ctl:ws_live".to_owned(),
                environment: expected,
            })
        );
    }
    for environment in ["", "Live", "demo"] {
        let refused = crate::control_record(&stored_row("ctl:ws_live", environment));
        assert!(matches!(refused, Err(PaperError::Control)), "{environment}");
    }
}

/// Journal spec v0.84, DEC-2131 item 4: an `AgentDeployed` version 3, its version 2's payload
/// with a trailing `step_up`, read from its stored row as the run's control read reads it, is
/// read by the door's deployment fold as its version 2: the door engages on the deployed
/// document and refuses the revoked connection exactly as it does for the earlier version.
#[test]
fn a_version_3_deployment_reads_as_its_version_2() -> Result<(), String> {
    let connection = || -> Result<Vec<ControlRecord>, String> {
        Ok(vec![
            record(
                2,
                "ConnectionEstablished",
                &format!(
                    r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper"}}"#
                ),
            )?,
            record(
                3,
                "ConnectionRevoked",
                r#"{"connection_id":"conn_alpaca_paper_01"}"#,
            )?,
        ])
    };
    let (args, mut earlier) = scene("version-2", MANDATE, WORKSPACE, connection()?)?;
    let (stepped_args, mut stepped) = scene("version-3", MANDATE, WORKSPACE, connection()?)?;
    let deployed = String::from_utf8(to_canonical(&stepped.records[0].payload))
        .map_err(|refused| refused.to_string())?;
    let open = deployed
        .strip_suffix('}')
        .ok_or_else(|| "a payload is an object".to_owned())?;
    let payload = format!(
        r#"{open},"connection_id":"conn_alpaca_paper_01","step_up":{{"assertion_id":"01JAA3P0A000000000000000H1","authenticated_at":"2026-10-10T15:04:57.000000000Z","method":"passkey"}}}}"#
    );
    let body = format!(
        r#"{{"event_time":"2026-10-10T15:05:00.000000000Z","event_type":"AgentDeployed","payload":{payload},"schema_version":3}}"#
    );
    let row = mandate_journal::StoredEvent {
        event_type: "AgentDeployed".to_owned(),
        schema_version: 3,
        seq: 1,
        body: body.into_bytes(),
        ..stored_row("ctl:ws_paper", "paper")
    };
    stepped.records[0] = crate::control_record(&row).map_err(|refused| refused.to_string())?;
    assert!(stepped.records[0].payload.get("step_up").is_some());
    let expected = run(&args, &[], &mut earlier)
        .err()
        .ok_or_else(|| "the revocation must refuse the earlier version's run".to_owned())?;
    let refused = run(&stepped_args, &[], &mut stepped)
        .err()
        .ok_or_else(|| "the revocation must refuse the version 3 run".to_owned())?;
    assert!(
        matches!(refused, PaperError::Connection(ConnectionRefusal::Revoked)),
        "{refused:?}"
    );
    assert_eq!(refused.to_string(), expected.to_string());
    assert_eq!(
        stepped.connects.get(),
        0,
        "no credential read before the door"
    );
    Ok(())
}
