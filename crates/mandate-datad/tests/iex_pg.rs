//! The IEX source over a real Postgres journal (DEC-2283 item 3): `serve` reads the workspace's
//! control stream through the journal and hands it to the worker, so a stream holding a row not of
//! the `paper` environment is read and refused as such, and the worker ends marked stopped. The
//! suite's PG class: it skips unless `MANDATE_PG_URL` is set, and fails without it where
//! `MANDATE_PG_REQUIRED` is (DEC-109, DEC-519). Nothing here carries a credential or reaches a host.

#[path = "../../mandate-journal/tests/common/mod.rs"]
#[allow(
    unused_imports,
    reason = "only the journal's support needs these; the suite runs in `mandate-journal-pg`"
)]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::cell::Cell as Counter;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};

use mandate_bars::generations::GENERATIONS_ROOT;
use mandate_datad::iex::pg::{Hosted, serve};
use mandate_datad::iex::progress::{Judged, Progress};
use mandate_datad::iex::worker::Edge;
use mandate_journal::StreamId;
use mandate_journal_pg::APP_ROLE;
use mandate_marketdata::http::Credentials;
use mandate_time::UtcNanos;

const WORKSPACE: &str = "ws_01J8Z2";

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The control stream's opening, as control services write it, in `environment`.
fn opened(environment: &str) -> Vec<u8> {
    let body = format!(
        r#"{{"envelope_version":1,"environment":"{environment}","event_id":"01J8Z2EVT0000000000000000A","stream_id":"ctl:{WORKSPACE}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"2026-10-17T15:00:00.000000000Z",
        "clock_source":"local","causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"control_services","version":"0.1.0","build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}},
        "config_refs":{{}},"payload":{{"stream_type":"control","workspace_id":"{WORKSPACE}"}},"artifact_refs":[],"pii_refs":[]}}"#
    );
    mandate_canon::to_canonical(&mandate_canon::parse(body.as_bytes()).unwrap())
}

#[test]
fn the_worker_reads_the_control_stream_through_postgres_and_ends_stopped() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let stream = StreamId::parse(&format!("ctl:{WORKSPACE}")).unwrap();
    let journal = db.journal();
    db.block_on(async {
        let epoch = journal.take_ownership(&stream).await.unwrap();
        journal
            .append(
                &stream,
                0,
                epoch,
                UtcNanos::parse_rfc3339("2026-10-17T15:00:00Z").unwrap(),
                &[&opened("live")],
            )
            .await
            .unwrap();
    });
    let root = std::env::temp_dir().join(format!("mandate-datad-pg-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(root.join("artifacts")).unwrap();
    mandate_artifacts_fs::FsArtifactStore::open(root.join("artifacts")).unwrap();
    let hosted = Hosted {
        workspace: WORKSPACE.to_owned(),
        store: root.join("datasets").join(GENERATIONS_ROOT),
        artifacts: root.join("artifacts"),
    };
    let terminate = AtomicBool::new(false);
    let progress = Progress::default();
    let turns = Counter::new(0u32);
    let mut log = Vec::new();
    let now = UtcNanos::parse_rfc3339("2026-10-27T14:30:10Z").unwrap();
    serve(
        &hosted,
        &dsn(&db),
        Credentials::new("PKTEST".into(), "s".into()),
        &terminate,
        &progress,
        Edge {
            now: &|| now,
            pause: &|| {
                turns.set(turns.get() + 1);
                terminate.store(true, Ordering::SeqCst);
            },
            log: &mut |line| log.push(line.to_owned()),
        },
    )
    .unwrap();
    assert_eq!(turns.get(), 1);
    assert_eq!(
        log,
        ["not_paper"],
        "the live row was read from Postgres and refused"
    );
    assert_eq!(
        progress.judged(now),
        Judged::Failed,
        "the worker ends stopped"
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_worker_that_cannot_build_its_parts_is_stopped() {
    let progress = Progress::default();
    let hosted = Hosted {
        workspace: "not a workspace".to_owned(),
        store: std::env::temp_dir().join(GENERATIONS_ROOT),
        artifacts: std::env::temp_dir(),
    };
    let served = serve(
        &hosted,
        "postgres://nowhere",
        Credentials::new("PKTEST".into(), "s".into()),
        &AtomicBool::new(false),
        &progress,
        Edge {
            now: &|| UtcNanos::EPOCH,
            pause: &|| {},
            log: &mut |_| {},
        },
    );
    assert_eq!(served, Err("workspace_invalid"));
    assert_eq!(progress.judged(UtcNanos::EPOCH), Judged::Failed);
}
