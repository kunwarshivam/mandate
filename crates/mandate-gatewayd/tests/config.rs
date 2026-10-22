//! The service's start-up values (E21-62, DEC-2272 item 7, DEC-79): every one is required and
//! none has a default, an empty one is missing, and each missing or malformed value stops the start
//! with its own code, the spending caps included.

use std::collections::BTreeMap;

use mandate_gateway::money::Usd6;
use mandate_gatewayd::config::{Config, ConfigError};

const NAMES: [(&str, ConfigError); 6] = [
    ("MANDATE_GATEWAY_WORKSPACE", ConfigError::WorkspaceMissing),
    ("MANDATE_GATEWAY_DEPLOYMENT", ConfigError::DeploymentMissing),
    ("MANDATE_JOURNAL_DSN", ConfigError::JournalMissing),
    ("MANDATE_ARTIFACTS", ConfigError::ArtifactsMissing),
    (
        "MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD",
        ConfigError::WorkspaceBudget,
    ),
    (
        "MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD",
        ConfigError::DeploymentBudget,
    ),
];

fn full() -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("MANDATE_GATEWAY_WORKSPACE", "ws_01J8Z2".to_owned()),
        ("MANDATE_GATEWAY_DEPLOYMENT", "dep_01J8Z0".to_owned()),
        (
            "MANDATE_JOURNAL_DSN",
            "postgres://gateway@journal/ws".to_owned(),
        ),
        ("MANDATE_ARTIFACTS", "/var/lib/mandate/artifacts".to_owned()),
        ("MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD", "50".to_owned()),
        (
            "MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD",
            "500.25".to_owned(),
        ),
    ])
}

fn read(env: &BTreeMap<&'static str, String>) -> Result<Config, ConfigError> {
    Config::from_env(&|name| env.get(name).cloned())
}

/// Every value named gives the config, each from its own variable.
#[test]
fn every_value_named_gives_the_config() {
    let config = read(&full()).unwrap();
    assert_eq!(config.workspace, "ws_01J8Z2");
    assert_eq!(config.deployment, "dep_01J8Z0");
    assert_eq!(config.journal_dsn(), "postgres://gateway@journal/ws");
    assert_eq!(config.artifacts, "/var/lib/mandate/artifacts");
    assert_eq!(config.workspace_monthly, Usd6::parse("50").unwrap());
    assert_eq!(config.deployment_monthly, Usd6::parse("500.25").unwrap());
}

/// GO's contract and DEC-79: each value unset or empty stops the start with its own code, and
/// nothing falls back on a default, `MANDATE_ARTIFACTS` and the two budgets included.
#[test]
fn each_value_unset_or_empty_stops_the_start_with_its_code() {
    for (name, error) in NAMES {
        let mut unset = full();
        unset.remove(name);
        assert_eq!(read(&unset).err(), Some(error), "{name} unset");
        let mut empty = full();
        empty.insert(name, String::new());
        assert_eq!(read(&empty).err(), Some(error), "{name} empty");
    }
    assert_eq!(
        NAMES.map(|(_, error)| error.code()),
        [
            "workspace_missing",
            "deployment_missing",
            "journal_dsn_missing",
            "artifact_store_unavailable",
            "workspace_budget_missing",
            "deployment_budget_missing",
        ]
    );
}

/// A budget that is not an amount `Usd6` holds stops the start rather than read as another.
#[test]
fn a_budget_that_is_not_an_amount_stops_the_start() {
    for (name, error) in &NAMES[4..] {
        for text in ["fifty", "-1", "1.0000001", "1e3", " 50"] {
            let mut env = full();
            env.insert(name, text.to_owned());
            assert_eq!(read(&env).err(), Some(*error), "{name}={text}");
        }
    }
}

/// Rule 7, the review of #2140: a DSN carrying a unique password shows in neither the config's
/// `Debug` nor any error's text, `Debug` or code, with every other value missing in turn.
#[test]
fn the_dsn_s_password_shows_nowhere() {
    const PASSWORD: &str = "pw-7c1e93b0-never-shown";
    let mut env = full();
    env.insert(
        "MANDATE_JOURNAL_DSN",
        format!("postgres://gateway:{PASSWORD}@journal:5432/ws"),
    );
    let config = read(&env).unwrap();
    assert!(
        config.journal_dsn().contains(PASSWORD),
        "the DSN itself is kept"
    );
    let shown = format!("{config:?}");
    assert!(!shown.contains(PASSWORD), "{shown}");
    assert!(!shown.contains("postgres://"), "{shown}");
    for (name, _) in NAMES {
        let mut missing = env.clone();
        missing.remove(name);
        if let Err(error) = read(&missing) {
            let shown = format!("{error} {error:?} {}", error.code());
            assert!(!shown.contains(PASSWORD), "{name}: {shown}");
        }
    }
}
