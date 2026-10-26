//! The executor reaches its in-process fixture or Alpaca's paper trading host, and nothing else
//! (E21-30, DEC-2306 item 1, `AGENTS.md` rule 8). `xtask/layers.toml` holds it: no external crate
//! of its own but `secrecy` and `tokio`; none of the crates that carry a transport to a host or a
//! simulator; and no `live` feature, which only the paper runner may declare. `mandate-alpaca`
//! carries the only paper transport, so the second test pins that one module of the executor,
//! [`CREDENTIAL_MODULE`], may name it, the credentials and the data client that reads quotes over
//! it (DEC-2359 item 5), and that no source names the HTTP client or a host. Loosening either
//! fails here as well as in review. DEC-2382 replaces DEC-2306 item 1 for the founder's own
//! Robinhood agentic account, and DEC-2386 item 1 lets the executor carry the Robinhood connector,
//! which names no host: `mandate-robinhood` is no longer forbidden, the default build still holds
//! no `live` feature, and no source names a host, the HTTP client, or the MCP transport that
//! reaches one. `mandate-mcp` is a dev-dependency only, for the tests' scripted tool seam
//! (DEC-2800), and never a dependency of the process.

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
fn the_executor_carries_no_transport_but_the_paper_hosts() -> Result<(), Error> {
    let entry = entry()?;
    assert!(!entry.is_empty(), "no mandate-executord entry");
    assert_eq!(
        value(&entry, "allowed_external").as_deref(),
        Some(r#"["secrecy", "tokio"]"#)
    );
    assert_eq!(value(&entry, "live_feature"), None);
    let forbidden = value(&entry, "forbidden_internal").ok_or("no forbidden_internal")?;
    for transport in ["mandate-paper", "mandate-shell", "mandate-rh-sim"] {
        assert!(
            forbidden.contains(&format!("\"{transport}\"")),
            "{transport} is reachable"
        );
    }
    Ok(())
}

/// Every line of the manifest that names `mandate-mcp`, with the table it sits in: a dependency
/// table's own header (`[dependencies.mandate-mcp]`, `[target.'cfg(unix)'.dependencies]`) or the
/// header of the table holding its line.
fn mcp_tables() -> Result<Vec<(String, String)>, Error> {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))?;
    let mut table = String::new();
    let mut found = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line.to_owned();
        }
        if line.contains("mandate-mcp") || line.contains("mandate_mcp") {
            found.push((table.clone(), line.to_owned()));
        }
    }
    Ok(found)
}

#[test]
fn the_process_never_depends_on_the_mcp_transport() -> Result<(), Error> {
    let tables = mcp_tables()?;
    assert!(
        !tables.is_empty(),
        "the tests' seam takes mandate-mcp as a dev-dependency"
    );
    for (table, line) in &tables {
        assert_eq!(
            table, "[dev-dependencies]",
            "mandate-mcp outside the dev-dependencies: {line}"
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

/// The one module that may name the paper transport, the credentials and the data client (DEC-2306
/// item 1, DEC-2359 item 5).
const CREDENTIAL_MODULE: &str = "credential.rs";

#[test]
fn only_the_credential_module_names_the_paper_transport() -> Result<(), Error> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = sources(&src)?;
    assert!(!files.is_empty(), "no sources under {}", src.display());
    for file in files {
        let text = fs::read_to_string(&file)?;
        for name in [
            "reqwest",
            "alpaca.markets",
            "robinhood.com",
            "https://",
            "http://",
            "PinnedEndpoint",
            "McpClient",
            "mandate_mcp",
        ] {
            assert!(!text.contains(name), "{} names {name}", file.display());
        }
        if file.file_name().is_some_and(|n| n == CREDENTIAL_MODULE) {
            continue;
        }
        for name in ["AlpacaPaperHttp", "Credentials", "DataClient"] {
            assert!(!text.contains(name), "{} names {name}", file.display());
        }
    }
    Ok(())
}

/// The `layer` of `crate_name`'s entry in `xtask/layers.toml`.
fn layer(crate_name: &str) -> Result<u32, Error> {
    let layers = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/layers.toml");
    let text = fs::read_to_string(layers)?;
    let header = format!("[crates.{crate_name}]");
    let entry: Vec<String> = text
        .lines()
        .skip_while(|line| line.trim() != header)
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(str::to_owned)
        .collect();
    Ok(value(&entry, "layer")
        .ok_or_else(|| format!("no layer for {crate_name}"))?
        .parse()?)
}

/// DEC-2306 item 6: the executor process sits above the vault, which it leases from, and the
/// connector crates sit below it, so neither can depend on it (a crate depends only on lower
/// layers, ES-02).
#[test]
fn only_the_process_sits_above_the_vault() -> Result<(), Error> {
    let vault = layer("mandate-vault")?;
    assert!(layer("mandate-executord")? > vault);
    for connector in ["mandate-executor", "mandate-alpaca"] {
        assert!(
            layer(connector)? < vault,
            "{connector} could take the vault"
        );
    }
    Ok(())
}

/// DEC-2359 item 9: the binary implements neither broker trait, so it cannot wrap a broker into
/// one the scripted serve path takes and serve the paper broker with a wiring that reads no quote.
#[test]
fn the_binary_implements_no_broker() -> Result<(), Error> {
    let main = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))?;
    let implementations: Vec<&str> = main
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("impl") && line.contains("Broker"))
        .collect();
    assert_eq!(implementations, Vec::<&str>::new());
    assert!(
        !main.contains("ScriptedBroker"),
        "src/main.rs names ScriptedBroker"
    );
    Ok(())
}
