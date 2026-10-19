//! Where an executor process learns its account (DEC-2007 item 3): from its flags, or from the
//! variables the deployment manager sets on the pod it starts (E21-33, design §3.5), ids only.

use std::collections::BTreeMap;

use mandate_executord::launch::{Named, named};

fn flags(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| map.get(name).cloned()
}

fn manager(process: &str) -> impl Fn(&str) -> Option<String> + use<> {
    flags(&[
        ("MANDATE_WORKSPACE", "ws1"),
        ("MANDATE_PROCESS", process),
        ("MANDATE_SUBJECT", "acct1"),
        ("MANDATE_GENERATION", "3"),
    ])
}

fn none() -> impl Fn(&str) -> Option<String> {
    flags(&[])
}

fn account() -> Named {
    Named {
        workspace: "ws1".into(),
        account: "acct1".into(),
    }
}

#[test]
fn the_flags_alone_name_an_account() {
    let given = flags(&[("--workspace", "ws1"), ("--account", "acct1")]);
    assert_eq!(named(&given, &none()), Ok(account()));
}

#[test]
fn a_launch_naming_no_workspace_or_account_is_refused() {
    for given in [
        flags(&[("--account", "acct1")]),
        flags(&[("--workspace", "ws1")]),
        flags(&[("--workspace", ""), ("--account", "acct1")]),
        flags(&[("--workspace", "ws1 "), ("--account", "acct1")]),
        flags(&[("--workspace", "ws1"), ("--account", " acct1")]),
    ] {
        assert_eq!(named(&given, &none()), Err("account_missing"));
    }
}

/// The deployment manager's pod: the workspace and the account from its variables, the generation
/// only informational (the writer epoch fences, DEC-2003); a flag that repeats a variable agrees.
#[test]
fn the_deployment_managers_variables_name_an_executor() {
    assert_eq!(named(&none(), &manager("executor")), Ok(account()));
    let repeated = flags(&[("--workspace", "ws1"), ("--account", "acct1")]);
    assert_eq!(named(&repeated, &manager("executor")), Ok(account()));
}

/// A pod started for another process, an unreadable process name, a flag that disagrees with the
/// manager's variable, and a manager variable set without `MANDATE_PROCESS` are each refused.
#[test]
fn a_launch_the_manager_did_not_start_for_an_executor_is_refused() {
    assert_eq!(
        named(&none(), &manager("runtime")),
        Err("process_not_executor")
    );
    assert_eq!(named(&none(), &manager("")), Err("process_not_executor"));
    let other = flags(&[("--account", "acct2")]);
    assert_eq!(named(&other, &manager("executor")), Err("account_conflict"));
    let other = flags(&[("--workspace", "ws2")]);
    assert_eq!(named(&other, &manager("executor")), Err("account_conflict"));
    for (name, value) in [("MANDATE_WORKSPACE", "ws1"), ("MANDATE_SUBJECT", "acct1")] {
        let stray = flags(&[(name, value)]);
        let given = flags(&[("--workspace", "ws1"), ("--account", "acct1")]);
        assert_eq!(named(&given, &stray), Err("account_conflict"), "{name}");
    }
}

/// The manager names an empty subject: no account is named, and none is guessed.
#[test]
fn a_manager_launch_with_an_empty_subject_names_no_account() {
    let vars = flags(&[
        ("MANDATE_WORKSPACE", "ws1"),
        ("MANDATE_PROCESS", "executor"),
        ("MANDATE_SUBJECT", ""),
    ]);
    assert_eq!(named(&none(), &vars), Err("account_missing"));
}
