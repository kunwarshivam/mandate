#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken policy file should stop the test"
)]
//! DP-8's type test: no text type reaches the order builder, the risk gate, the executor, or
//! accounting, because none of them can depend on this crate. `cargo xtask layers` enforces the
//! layers; this test pins that this crate's layer stays above theirs.

use std::path::PathBuf;

fn layer_of(policy: &toml::Table, name: &str) -> i64 {
    policy["crates"][name]["layer"]
        .as_integer()
        .unwrap_or_else(|| panic!("{name} has no integer layer"))
}

#[test]
fn the_order_path_cannot_reach_untrusted_text() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../xtask/layers.toml");
    let policy: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let own = layer_of(&policy, "mandate-sources");
    assert_eq!(
        policy["crates"]["mandate-sources"]["safety_critical"].as_bool(),
        Some(true)
    );
    for order_path in [
        "mandate-builder",
        "mandate-risk",
        "mandate-executor",
        "mandate-accounting",
        "mandate-runtime",
        "mandate-research",
    ] {
        assert!(
            layer_of(&policy, order_path) <= own,
            "{order_path} sits above mandate-sources and could depend on it"
        );
    }
}
