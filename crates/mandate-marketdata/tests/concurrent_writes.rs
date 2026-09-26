//! Writers of one dataset running at the same time, as threads and as processes, never tear or
//! replace a partition, never reuse or publish a crashed write's leftover, and never lose a
//! manifest entry; a re-run still changes no file (DEC-89, the E2-1 follow-up in the work tracker).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Barrier;
use std::thread;
use std::time::SystemTime;

use common::{Scratch, day, spy_sip_1hour};
use mandate_canon::{DecStr, Digest};
use mandate_marketdata::dataset::{self, DatasetError, ListedDay, Outcome, Status, Store};
use mandate_marketdata::inspect::inspect;
use mandate_marketdata::model::{Bar, DatasetId, Records};
use mandate_time::{Date, UtcNanos};

const WRITERS: usize = 8;
const ROUNDS: usize = 16;
const DAYS_PER_WRITER: usize = 6;
const PROCESSES: usize = 4;
const PROCESS_DAYS: usize = 12;
const CHILD_ROOT: &str = "MANDATE_MARKETDATA_TEST_WRITER_ROOT";
const CHILD_INDEX: &str = "MANDATE_MARKETDATA_TEST_WRITER_INDEX";

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

/// Four hourly bars of `day`; a different `open` gives a different partition.
fn bars(day: Date, open: &str) -> Records {
    Records::Bars(
        (13..17)
            .map(|hour| Bar {
                start: UtcNanos::parse_rfc3339(&format!("{day}T{hour}:00:00Z")).unwrap(),
                open: dec(open),
                high: dec("765.5"),
                low: dec("763.25"),
                close: dec("764.125"),
                volume: dec("1200"),
                vwap: dec("764.0625"),
                trade_count: 17,
            })
            .collect(),
    )
}

fn days_from(first: &str, count: usize) -> Vec<Date> {
    let mut days = vec![day(first)];
    while days.len() < count {
        let next = days.last().unwrap().next().unwrap();
        days.push(next);
    }
    days
}

/// Every other day has records; the rest are fetched days without any.
fn records_of(index: usize, day: Date) -> Records {
    if index.is_multiple_of(2) {
        bars(day, "764.53")
    } else {
        Records::empty(spy_sip_1hour().kind())
    }
}

/// What the manifest must list for `days`, computed from the encoder rather than the store.
fn expected_listing(id: &DatasetId, days: &[(Date, Records)]) -> Vec<ListedDay> {
    let mut listing: Vec<ListedDay> = days
        .iter()
        .map(|(day, records)| ListedDay {
            day: *day,
            rows: records.len() as u64,
            file: (!records.is_empty()).then(|| {
                let bytes = dataset::encode(id, records).unwrap();
                (bytes.len() as u64, Digest::of(&bytes))
            }),
        })
        .collect();
    listing.sort_by_key(|listed| listed.day);
    listing
}

fn names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn mtimes(dir: &Path) -> BTreeMap<String, SystemTime> {
    names(dir)
        .into_iter()
        .map(|name| {
            let modified = fs::metadata(dir.join(&name)).unwrap().modified().unwrap();
            (name, modified)
        })
        .collect()
}

/// The partitions and the manifest that `listing` implies, and nothing else.
fn expected_names(listing: &[ListedDay]) -> BTreeSet<String> {
    listing
        .iter()
        .filter(|listed| listed.file.is_some())
        .map(|listed| dataset::partition_name(listed.day))
        .chain([dataset::MANIFEST.to_owned()])
        .collect()
}

/// Runs `writer(i)` for every `i` below `count` on its own thread, all released at once.
fn race<R: Send>(count: usize, writer: impl Fn(usize) -> R + Sync) -> Vec<R> {
    let start = Barrier::new(count);
    thread::scope(|scope| {
        let handles: Vec<_> = (0..count)
            .map(|i| {
                let (start, writer) = (&start, &writer);
                scope.spawn(move || {
                    start.wait();
                    writer(i)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

fn status_of(outcome: &Result<Outcome, DatasetError>) -> Result<Status, String> {
    outcome
        .as_ref()
        .map(|outcome| outcome.status)
        .map_err(ToString::to_string)
}

#[test]
fn concurrent_writers_of_one_partition_store_it_once_and_whole() {
    let id = spy_sip_1hour();
    let d = day("2026-09-24");
    let records = bars(d, "764.53");
    let bytes = dataset::encode(&id, &records).unwrap();
    let listing = expected_listing(&id, &[(d, records.clone())]);
    for round in 0..ROUNDS {
        let scratch = Scratch::new("one-partition");
        let store = Store::new(scratch.path());
        let statuses: Vec<_> = race(WRITERS, |_| store.put_day(&id, d, &records))
            .iter()
            .map(status_of)
            .collect();
        let mut expected = vec![Ok(Status::Unchanged); WRITERS];
        expected[0] = Ok(Status::Written);
        let mut sorted = statuses.clone();
        sorted.sort_by_key(|s| s != &Ok(Status::Written));
        assert_eq!(
            sorted, expected,
            "round {round}: one writer stores the partition and the others find it"
        );
        let dir = store.dataset_dir(&id);
        let partition = dir.join(dataset::partition_name(d));
        assert_eq!(fs::read(&partition).unwrap(), bytes, "round {round}");
        assert_eq!(dataset::read(&partition, id.kind()).unwrap(), records);
        assert_eq!(dataset::read_manifest(&dir).unwrap(), (id.clone(), listing.clone()));
        assert_eq!(
            names(&dir),
            expected_names(&listing),
            "round {round}: no temporary file is left behind"
        );
        let before = mtimes(&dir);
        assert_eq!(
            status_of(&store.put_day(&id, d, &records)),
            Ok(Status::Unchanged)
        );
        assert_eq!(mtimes(&dir), before, "a re-run changes no file");
    }
}

#[test]
fn concurrent_writers_with_different_data_keep_the_first_and_report_conflicts() {
    let id = spy_sip_1hour();
    let d = day("2026-09-24");
    let versions: Vec<Records> = (0..WRITERS)
        .map(|i| bars(d, &format!("764.5{i}")))
        .collect();
    for round in 0..ROUNDS {
        let scratch = Scratch::new("disagreeing");
        let store = Store::new(scratch.path());
        let outcomes = race(WRITERS, |i| store.put_day(&id, d, &versions[i]));
        let winners: Vec<usize> = (0..WRITERS)
            .filter(|i| status_of(&outcomes[*i]) == Ok(Status::Written))
            .collect();
        assert_eq!(winners.len(), 1, "round {round}: {outcomes:?}");
        let winner = winners[0];
        for (i, outcome) in outcomes.iter().enumerate() {
            if i != winner {
                assert!(
                    matches!(outcome, Err(DatasetError::Conflict { .. })),
                    "round {round} writer {i}: {outcome:?}"
                );
            }
        }
        let dir = store.dataset_dir(&id);
        assert_eq!(
            fs::read(dir.join(dataset::partition_name(d))).unwrap(),
            dataset::encode(&id, &versions[winner]).unwrap(),
            "round {round}: the stored partition is the first writer's, never replaced"
        );
        let listing = expected_listing(&id, &[(d, versions[winner].clone())]);
        assert_eq!(dataset::read_manifest(&dir).unwrap(), (id.clone(), listing.clone()));
        assert_eq!(names(&dir), expected_names(&listing), "round {round}");
    }
}

#[test]
fn concurrent_writers_of_different_days_lose_no_manifest_entry() {
    let id = spy_sip_1hour();
    let all = days_from("2026-01-01", WRITERS * DAYS_PER_WRITER);
    let planned: Vec<(Date, Records)> = all
        .iter()
        .enumerate()
        .map(|(i, d)| (*d, records_of(i, *d)))
        .collect();
    let listing = expected_listing(&id, &planned);
    for round in 0..ROUNDS / 4 {
        let scratch = Scratch::new("many-days");
        let store = Store::new(scratch.path());
        let statuses = race(WRITERS, |w| {
            planned
                .iter()
                .skip(w * DAYS_PER_WRITER)
                .take(DAYS_PER_WRITER)
                .map(|(d, records)| status_of(&store.put_day(&id, *d, records)))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            statuses,
            vec![vec![Ok(Status::Written); DAYS_PER_WRITER]; WRITERS],
            "round {round}"
        );
        let dir = store.dataset_dir(&id);
        assert_eq!(
            dataset::read_manifest(&dir).unwrap(),
            (id.clone(), listing.clone()),
            "round {round}: every day any writer stored is listed"
        );
        assert_eq!(names(&dir), expected_names(&listing), "round {round}");
    }
}

#[test]
fn concurrent_processes_writing_one_dataset_agree() {
    let id = spy_sip_1hour();
    let scratch = Scratch::new("processes");
    let children: Vec<_> = (0..PROCESSES)
        .map(|index| {
            Command::new(env::current_exe().unwrap())
                .args(["--exact", "writer_process", "--nocapture"])
                .env(CHILD_ROOT, scratch.path())
                .env(CHILD_INDEX, index.to_string())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success(), "a writer process failed");
    }
    let planned: Vec<(Date, Records)> = days_from("2026-03-02", PROCESS_DAYS)
        .into_iter()
        .enumerate()
        .map(|(i, d)| (d, records_of(i, d)))
        .collect();
    let listing = expected_listing(&id, &planned);
    let dir = Store::new(scratch.path()).dataset_dir(&id);
    assert_eq!(dataset::read_manifest(&dir).unwrap(), (id.clone(), listing.clone()));
    assert_eq!(names(&dir), expected_names(&listing));
    for (d, records) in planned.iter().filter(|(_, r)| !r.is_empty()) {
        let partition = dir.join(dataset::partition_name(*d));
        assert_eq!(dataset::read(&partition, id.kind()).unwrap(), *records);
    }
    let mut written: BTreeMap<String, usize> = BTreeMap::new();
    for index in 0..PROCESSES {
        let report = fs::read_to_string(scratch.path().join(format!("writer-{index}.txt"))).unwrap();
        for line in report.lines() {
            let (d, status) = line.split_once(' ').unwrap();
            assert!(status == "Written" || status == "Unchanged", "{line}");
            *written.entry(d.to_owned()).or_default() += usize::from(status == "Written");
        }
    }
    assert_eq!(
        written,
        planned
            .iter()
            .map(|(d, _)| (d.to_string(), 1))
            .collect::<BTreeMap<_, _>>(),
        "each day is written by exactly one process"
    );
}

/// The body of each child that `concurrent_processes_writing_one_dataset_agree` starts: it stores
/// every planned day, starting at a day that depends on its index so that the processes collide
/// on partitions and on the manifest, and reports each outcome. Run directly, it does nothing.
#[test]
fn writer_process() {
    let (Some(root), Some(index)) = (env::var_os(CHILD_ROOT), env::var_os(CHILD_INDEX)) else {
        return;
    };
    let root = PathBuf::from(root);
    let index: usize = index.to_str().unwrap().parse().unwrap();
    let id = spy_sip_1hour();
    let store = Store::new(&root);
    let days = days_from("2026-03-02", PROCESS_DAYS);
    let mut report = String::new();
    for k in 0..PROCESS_DAYS {
        let i = (k + index * PROCESS_DAYS / PROCESSES) % PROCESS_DAYS;
        let outcome = store.put_day(&id, days[i], &records_of(i, days[i]));
        let status = status_of(&outcome).unwrap_or_else(|e| panic!("{}: {e}", days[i]));
        report.push_str(&format!("{} {status:?}\n", days[i]));
    }
    fs::write(root.join(format!("writer-{index}.txt")), report).unwrap();
}

#[test]
fn a_crashed_writes_leftovers_are_neither_reused_nor_published() {
    let id = spy_sip_1hour();
    let d = day("2026-09-24");
    let records = bars(d, "764.53");
    let bytes = dataset::encode(&id, &records).unwrap();
    let scratch = Scratch::new("leftovers");
    let store = Store::new(scratch.path());
    let dir = store.dataset_dir(&id);
    fs::create_dir_all(&dir).unwrap();
    let torn = [&bytes[..bytes.len() / 2], &[0xAB; 4096]].concat();
    let mut leftovers = BTreeMap::new();
    for target in [dataset::partition_name(d), dataset::MANIFEST.to_owned()] {
        leftovers.insert(format!("{target}.partial"), torn.clone());
        for n in 0..1024 {
            let name = format!("{target}.{}.{n}.partial", std::process::id());
            leftovers.insert(name, torn.clone());
        }
    }
    for (name, content) in &leftovers {
        fs::write(dir.join(name), content).unwrap();
    }

    assert_eq!(
        status_of(&store.put_day(&id, d, &records)),
        Ok(Status::Written),
        "a leftover does not block the write"
    );
    let partition = dir.join(dataset::partition_name(d));
    assert_eq!(fs::read(&partition).unwrap(), bytes, "a leftover is never published");
    let listing = expected_listing(&id, &[(d, records.clone())]);
    assert_eq!(dataset::read_manifest(&dir).unwrap(), (id.clone(), listing.clone()));
    for (name, content) in &leftovers {
        assert_eq!(&fs::read(dir.join(name)).unwrap(), content, "{name} is never reused");
    }
    let expected: BTreeSet<String> = expected_names(&listing)
        .into_iter()
        .chain(leftovers.keys().cloned())
        .collect();
    assert_eq!(names(&dir), expected);
    assert_eq!(
        inspect(&dir).unwrap().problems,
        vec![],
        "leftovers are not taken for partitions"
    );
    assert_eq!(
        status_of(&store.put_day(&id, d, &records)),
        Ok(Status::Unchanged)
    );
}
