//! E12-6 slice A1: tenant scoping of the journal reads and the stream list (workspace API §4.8.1,
//! API-9, AU-1, DEC-760 items 4 to 6, DEC-770). The oracles read what the fixture appended and the journal's own
//! rows; none calls the code under test to decide what it should have returned.

mod common;

use common::{
    ACCOUNTS, Fixture, T, WS_A, WS_B, acct, context, ctl, event_id, limit, permitted, tenant, text,
    workspaces,
};
use mandate_audit::{AuditError, JournalRead, MemoryRead, PageLimit, StreamType};
use mandate_canon::Digest;
use mandate_identity::demand::ReadRecords;
use mandate_identity::{Permission, Refusal, Role, WorkspaceId};
use mandate_journal::StreamId;
use proptest::prelude::*;

/// The workspaces a tenant context is built for. The fixture's second segment, `WS_A`'s with one
/// more character, is no workspace's, so no tenant reads it.
const TENANTS: [WorkspaceId; 2] = [WS_A, WS_B];

/// A hosted workspace that holds no stream.
const WS_EMPTY: WorkspaceId = WorkspaceId(0x0192_0C3A_7F10_4B2E_9D01_0000_0000_00E3);

/// [`WS_A`]'s segment with one more character: a stream there is never `WS_A`'s.
fn prefixed() -> String {
    workspaces()[1].clone()
}

#[test]
fn streams_lists_only_the_workspaces_written_streams_with_their_heads() {
    let mut fx = Fixture::new(2);
    fx.marks(&acct(&text(WS_A), "ACCT2"), 3);
    let never_written = StreamId::parse(&acct(&text(WS_A), "EMPTY")).unwrap();
    fx.journal.take_ownership(&never_written);
    let read = MemoryRead::new(&fx.journal);
    let all = PageLimit::new(None).unwrap();
    for workspace in TENANTS {
        let entries = read
            .streams(&permitted(&tenant(workspace)), None, all)
            .unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.stream_id.as_str()).collect();
        assert_eq!(names, fx.streams_of(&text(workspace)), "{workspace:?}");
        for entry in &entries {
            let rows = fx.journal.rows(&StreamId::parse(&entry.stream_id).unwrap());
            let last = rows.last().unwrap();
            let expected_type = match entry.stream_id.split(':').next() {
                Some("acct") => StreamType::Account,
                Some("ctl") => StreamType::Control,
                other => panic!("the fixture opens no {other:?} stream"),
            };
            assert_eq!(entry.stream_type, expected_type, "{}", entry.stream_id);
            assert_eq!(entry.head.seq, rows.len() as u64, "{}", entry.stream_id);
            assert_eq!(
                entry.head.hash,
                Digest::of(&last.body),
                "{}",
                entry.stream_id
            );
            assert_eq!(entry.head.recorded_at, T);
        }
    }
    assert_eq!(
        read.streams(&permitted(&tenant(WS_EMPTY)), None, all)
            .unwrap(),
        []
    );
}

/// DEC-760 item 6: the list pages by `after` and `limit` in byte order. `after` is a cursor, not
/// an id to resolve, so another workspace's stream id there only says where to start.
#[test]
fn the_stream_list_pages_by_after_and_limit() {
    let fx = Fixture::new(1);
    let read = MemoryRead::new(&fx.journal);
    let context = tenant(WS_A);
    let me = permitted(&context);
    let expected = fx.streams_of(&text(WS_A));
    let mut after: Option<String> = None;
    let mut listed = Vec::new();
    for _ in 0..=expected.len() {
        let page = read.streams(&me, after.as_deref(), limit(1)).unwrap();
        assert!(page.len() <= 1);
        let Some(entry) = page.into_iter().next() else {
            break;
        };
        after = Some(entry.stream_id.clone());
        listed.push(entry.stream_id);
    }
    assert_eq!(
        listed, expected,
        "pages of one, each after the last, list each stream once"
    );
    let two = read.streams(&me, None, limit(2)).unwrap();
    assert_eq!(two.len(), 2);
    let names = |after: &str| -> Vec<String> {
        read.streams(&me, Some(after), limit(1000))
            .unwrap()
            .into_iter()
            .map(|e| e.stream_id)
            .collect()
    };
    assert_eq!(names(""), expected);
    assert_eq!(names(&acct(&text(WS_A), "ACCT1")), expected[1..]);
    assert_eq!(names(&acct(&text(WS_B), "ACCT1")), [ctl(&text(WS_A))]);
    assert_eq!(names(&acct(&prefixed(), "ACCT1")), expected);
    assert_eq!(names("zzz"), Vec::<String>::new());
}

/// DEC-655, DEC-770 item 4: the reads take only the `ReadRecords` witness, and a context
/// authorized for another permission, here a Viewer's `ViewAgents`, is refused it, so it reaches
/// no read. That every other row is refused is identity's own test.
#[test]
fn a_context_for_another_permission_reaches_no_read() {
    let viewer = context(WS_A, Role::Viewer, Permission::ViewAgents);
    assert_eq!(
        viewer.require::<ReadRecords>().err(),
        Some(Refusal::Forbidden)
    );
}

#[test]
fn foreign_malformed_and_absent_ids_all_read_as_not_found() {
    let mut fx = Fixture::new(1);
    let a = text(WS_A);
    fx.journal
        .take_ownership(&StreamId::parse(&acct(&a, "EMPTY")).unwrap());
    let read = MemoryRead::new(&fx.journal);
    let context = tenant(WS_A);
    let me = permitted(&context);
    for stream in [
        acct(&prefixed(), "ACCT1"),
        acct(&text(WS_B), "ACCT2"),
        ctl(&text(WS_B)),
        ctl(&prefixed()),
        ctl(&a.to_lowercase()),
        acct(&a, "ACCT9"),
        acct(&a, "EMPTY"),
        format!("acct:{a}"),
        format!("ctl:{a}:x"),
        format!(" ctl:{a}"),
        format!("ctl:{a} "),
        format!("CTL:{a}"),
        String::new(),
    ] {
        for after in [0, 1] {
            assert_eq!(
                read.page(&me, &stream, after, limit(5)),
                Err(AuditError::NotFound),
                "{stream:?}"
            );
        }
    }
    for stream in [
        acct(&text(WS_B), "ACCT1"),
        acct(&a, "ACCT9"),
        acct(&a, "ACCT1"),
    ] {
        assert_eq!(
            read.page(&me, &stream, 9_007_199_254_740_992, limit(5)),
            Err(AuditError::AfterSeqOutOfRange {
                after_seq: 9_007_199_254_740_992
            }),
            "a query member that is not an id is checked before the id ({stream})"
        );
    }
    let foreign = fx.appended[&acct(&text(WS_B), "ACCT1")].event_ids[0].clone();
    let neighbour = fx.appended[&ctl(&prefixed())].event_ids[0].clone();
    for id in [
        foreign,
        neighbour,
        event_id(999_999),
        "x".to_owned(),
        String::new(),
    ] {
        assert_eq!(read.event(&me, &id), Err(AuditError::NotFound), "{id:?}");
    }
}

#[test]
fn an_event_of_the_workspace_is_served_with_its_body_and_link() {
    let fx = Fixture::new(4);
    let read = MemoryRead::new(&fx.journal);
    for workspace in TENANTS {
        for account in ACCOUNTS {
            let stream = acct(&text(workspace), account);
            let rows = fx.journal.rows(&StreamId::parse(&stream).unwrap());
            for (i, id) in fx.appended[&stream].event_ids.iter().enumerate() {
                let event = read.event(&permitted(&tenant(workspace)), id).unwrap();
                assert_eq!(event.event_id, *id);
                assert_eq!(event.stream_id, stream);
                assert_eq!(event.seq, i as u64 + 1);
                assert_eq!(event.event_type, rows[i].event_type);
                assert_eq!(event.recorded_at, T);
                assert_eq!(event.body, rows[i].body);
                assert_eq!(event.hash, Digest::of(&rows[i].body));
                let prev = i
                    .checked_sub(1)
                    .map_or(Digest::ZERO, |p| Digest::of(&rows[p].body));
                assert_eq!(event.prev_hash, prev);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// API-9: as any workspace, every stream and event of another workspace reads exactly as an
    /// absent one does, and every one of its own reads.
    #[test]
    fn foreign_and_absent_reads_are_indistinguishable(
        reader in 0..TENANTS.len(),
        marks in 0..4u64,
        after in 0..6u64,
        l in 1..=1000u64,
        absent in 1_000..2_000u64,
    ) {
        let fx = Fixture::new(marks);
        let read = MemoryRead::new(&fx.journal);
        let mine_text = text(TENANTS[reader]);
        let context = tenant(TENANTS[reader]);
        let me = permitted(&context);
        let absent_page = read.page(&me, &acct(&mine_text, &format!("A{absent}")), after, limit(l));
        let absent_event = read.event(&me, &event_id(absent * 1_000));
        prop_assert_eq!(&absent_page, &Err(AuditError::NotFound));
        prop_assert_eq!(&absent_event, &Err(AuditError::NotFound));
        for (stream, appended) in &fx.appended {
            let mine = stream.split(':').nth(1) == Some(mine_text.as_str());
            let page = read.page(&me, stream, after, limit(l));
            let first = appended.event_ids.first().unwrap();
            let event = read.event(&me, first);
            if mine {
                prop_assert!(page.is_ok(), "{} {:?}", stream, page);
                prop_assert_eq!(event.map(|e| e.event_id), Ok(first.clone()));
            } else {
                prop_assert_eq!(&page, &absent_page, "{}", stream);
                prop_assert_eq!(&event, &absent_event, "{}", stream);
                prop_assert_eq!(
                    page.unwrap_err().to_string(),
                    absent_page.clone().unwrap_err().to_string()
                );
            }
        }
    }
}
