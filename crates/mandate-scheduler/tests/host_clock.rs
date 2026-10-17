//! Design OPS-14 and E21-29's criterion: a host clock out of tolerance never adds risk, because no
//! order path consumes the host clock. The order path reads time only as the scheduler's ticks
//! and the injected clock the shell hands it; this test holds that none of its crates reads a
//! clock itself or excuses itself from clippy's `disallowed_methods` (ES-05), so a host clock
//! that is wrong cannot reach a risk decision except through a tick, which the scheduler
//! journals and measures.

use std::fs;
use std::path::{Path, PathBuf};

/// The crates an order passes through, from decision to the journaled request: the runtime, the
/// builder, the gate and the account rules, accounting, approvals, and the executor.
const ORDER_PATH: [&str; 9] = [
    "mandate-runtime",
    "mandate-builder",
    "mandate-risk",
    "mandate-spec",
    "mandate-accounting",
    "mandate-approval",
    "mandate-executor",
    "mandate-domain",
    "mandate-liquidity",
];

/// The ways Rust code can read a host clock, or excuse itself from the lint that forbids it.
const CLOCK_READS: [&str; 5] = [
    "SystemTime::now",
    "Instant::now",
    "Utc::now",
    "Local::now",
    "disallowed_methods",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_order_path_crate_reads_the_host_clock() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut found = Vec::new();
    for name in ORDER_PATH {
        let mut files = Vec::new();
        sources(&crates.join(name).join("src"), &mut files);
        assert!(!files.is_empty(), "{name} has sources");
        for file in files {
            let text = fs::read_to_string(&file).unwrap();
            for read in CLOCK_READS {
                if text.contains(read) {
                    found.push(format!("{}: {read}", file.display()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "host clock reads on the order path:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_finds_a_clock_read_when_one_is_there() {
    let dir = std::env::temp_dir().join(format!("host-clock-scan-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("lib.rs"),
        "fn f() { let _ = std::time::Instant::now(); }",
    )
    .unwrap();
    let mut files = Vec::new();
    sources(&dir, &mut files);
    let text = fs::read_to_string(&files[0]).unwrap();
    assert!(CLOCK_READS.iter().any(|read| text.contains(read)));
    fs::remove_dir_all(&dir).unwrap();
}
