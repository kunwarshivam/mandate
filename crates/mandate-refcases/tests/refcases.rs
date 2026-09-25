//! One named test per reference case. Run pending cases with `-- --include-ignored`.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use libtest_mimic::{Arguments, Failed, Trial};
use mandate_refcases::{Case, CaseStatus, journal, parse_status, read_fixture};

fn main() -> ExitCode {
    let args = Arguments::from_args();
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = crate_dir.join("../../fixtures/refcases");

    let mut setup_errors = Vec::new();
    let status = match std::fs::read_to_string(crate_dir.join("status.toml")) {
        Ok(text) => parse_status(&text).unwrap_or_else(|e| {
            setup_errors.push(e);
            Default::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => {
            setup_errors.push(format!("status.toml: {e}"));
            Default::default()
        }
    };

    let mut cases: Vec<Case> = Vec::new();
    match read_fixture(&fixtures, "journal.json") {
        Ok(fixture) => cases.extend(journal::cases(&fixture)),
        Err(e) => setup_errors.push(e),
    }

    let ids: BTreeSet<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    for listed in status.keys().filter(|id| !ids.contains(id.as_str())) {
        setup_errors.push(format!("status.toml lists `{listed}`, which is not a case"));
    }
    if ids.len() != cases.len() {
        setup_errors.push("two cases share an ID".to_owned());
    }

    let mut trials: Vec<Trial> = setup_errors
        .into_iter()
        .enumerate()
        .map(|(i, e)| Trial::test(format!("setup::{i}"), move || Err(Failed::from(e))))
        .collect();
    for case in cases {
        let pending = status.get(&case.id) != Some(&CaseStatus::Passing);
        let run = case.run;
        trials.push(
            Trial::test(case.id, move || run().map_err(Failed::from))
                .with_ignored_flag(pending)
                .with_kind(if pending { "pending" } else { "" }),
        );
    }
    libtest_mimic::run(&args, trials).exit_code()
}
