//! The in-process fixture is the only broker the executor can reach (E21-30, DEC-2033 item 7,
//! `AGENTS.md` rule 8). `xtask/layers.toml` holds it: no external crate of its own but `tokio`;
//! none of the crates that carry a transport to a host or a simulator; and no `live` feature, which
//! only the paper runner may declare. `mandate-alpaca` links `reqwest` for its paper HTTPS
//! transport, so the second test pins that no source of the executor names that transport or the
//! HTTP client. Loosening either fails here as well as in review.

use std::fs;
use std::path::{Path, PathBuf};

type Error = Box<dyn std::error::Error>;

/// The `[crates.mandate-executord]` entry's lines, up to the next entry.
fn entry() -> Result<Vec<String>, Error> {
    let layers = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/layers.toml");
    let text = fs::read_to_string(layers)?;
    Ok(text
        .lines()
        .skip_while(|line| line.trim() != "[crates.mandate-executord]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(str::to_owned)
        .collect())
}

fn value(entry: &[String], key: &str) -> Option<String> {
    entry.iter().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

#[test]
fn the_executor_reaches_no_broker_but_its_in_process_fixture() -> Result<(), Error> {
    let entry = entry()?;
    assert!(!entry.is_empty(), "no mandate-executord entry");
    assert_eq!(
        value(&entry, "allowed_external").as_deref(),
        Some(r#"["tokio"]"#)
    );
    assert_eq!(value(&entry, "live_feature"), None);
    let forbidden = value(&entry, "forbidden_internal").ok_or("no forbidden_internal")?;
    for transport in [
        "mandate-paper",
        "mandate-shell",
        "mandate-robinhood",
        "mandate-rh-sim",
    ] {
        assert!(
            forbidden.contains(&format!("\"{transport}\"")),
            "{transport} is reachable"
        );
    }
    Ok(())
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            found.extend(sources(&path)?);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    Ok(found)
}

#[test]
fn the_executor_names_no_http_transport() -> Result<(), Error> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = sources(&src)?;
    assert!(!files.is_empty(), "no sources under {}", src.display());
    for file in files {
        let text = fs::read_to_string(&file)?;
        for name in ["AlpacaPaperHttp", "reqwest", "DataClient", "Credentials"] {
            assert!(!text.contains(name), "{} names {name}", file.display());
        }
    }
    Ok(())
}
