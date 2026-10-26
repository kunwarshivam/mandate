//! Where a runtime process learns its deployment (DEC-1015): from its flags, or from the
//! variables the deployment manager sets on the pod it starts (E21-33), ids only.

use std::collections::BTreeMap;

use mandate_daemon::launch::{self, Named, fixture_permitted, named};

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
        ("MANDATE_SUBJECT", "agent-a"),
        ("MANDATE_GENERATION", "3"),
    ])
}

fn none() -> impl Fn(&str) -> Option<String> {
    flags(&[])
}

fn whole() -> Named {
    Named {
        workspace: "ws1".into(),
        agent: "agent-a".into(),
        connection: Some("conn-1".into()),
        owner: Some("user-owner".into()),
    }
}

#[test]
fn the_flags_alone_name_a_deployment() {
    let given = flags(&[
        ("--workspace", "ws1"),
        ("--agent", "agent-a"),
        ("--connection", "conn-1"),
        ("--owner", "user-owner"),
    ]);
    assert_eq!(named(&given, &none()), Ok(whole()));
}

#[test]
fn a_launch_without_its_connection_and_owner_leaves_them_to_be_resolved() {
    let given = flags(&[("--workspace", "ws1"), ("--agent", "agent-a")]);
    let found = named(&given, &none()).unwrap();
    assert_eq!(found.connection, None);
    assert_eq!(found.owner, None);
    assert_eq!(found.agent, "agent-a");
    assert_eq!(found.workspace, "ws1");
}

#[test]
fn a_launch_naming_no_workspace_or_agent_is_refused() {
    let only_agent = flags(&[("--agent", "agent-a")]);
    assert_eq!(named(&only_agent, &none()), Err("deployment_missing"));
    let only_workspace = flags(&[("--workspace", "ws1")]);
    assert_eq!(named(&only_workspace, &none()), Err("deployment_missing"));
    let empty = flags(&[("--workspace", ""), ("--agent", "agent-a")]);
    assert_eq!(named(&empty, &none()), Err("deployment_missing"));
    let padded = flags(&[("--workspace", "ws1 "), ("--agent", "agent-a")]);
    assert_eq!(named(&padded, &none()), Err("deployment_missing"));
}

/// The deployment manager's pod: the workspace and the agent from its variables, the generation
/// only informational (the writer epoch fences, DEC-1012).
#[test]
fn the_deployment_manager_s_variables_name_a_runtime() {
    let found = named(&none(), &manager("runtime")).unwrap();
    assert_eq!(found.workspace, "ws1");
    assert_eq!(found.agent, "agent-a");
    assert_eq!(found.connection, None);
    let given = flags(&[("--connection", "conn-1"), ("--owner", "user-owner")]);
    assert_eq!(named(&given, &manager("runtime")), Ok(whole()));
}

#[test]
fn a_pod_started_for_another_process_is_refused() {
    for process in ["executor", "", "Runtime", " runtime"] {
        assert_eq!(
            named(&none(), &manager(process)),
            Err("process_not_runtime"),
            "{process:?}"
        );
    }
}

#[test]
fn a_manager_launch_missing_a_variable_is_refused() {
    let no_subject = flags(&[("MANDATE_WORKSPACE", "ws1"), ("MANDATE_PROCESS", "runtime")]);
    assert_eq!(named(&none(), &no_subject), Err("deployment_missing"));
    let no_workspace = flags(&[
        ("MANDATE_SUBJECT", "agent-a"),
        ("MANDATE_PROCESS", "runtime"),
    ]);
    assert_eq!(named(&none(), &no_workspace), Err("deployment_missing"));
    let blank = flags(&[
        ("MANDATE_WORKSPACE", "ws1"),
        ("MANDATE_PROCESS", "runtime"),
        ("MANDATE_SUBJECT", ""),
    ]);
    assert_eq!(named(&none(), &blank), Err("deployment_missing"));
}

/// A flag never overrides what the manager named: one that disagrees is refused rather than
/// guessed between (`AGENTS.md` rule 3); one that agrees is accepted.
#[test]
fn a_flag_that_disagrees_with_the_manager_is_refused() {
    let agent = flags(&[("--agent", "agent-b")]);
    assert_eq!(
        named(&agent, &manager("runtime")),
        Err("deployment_conflict")
    );
    let workspace = flags(&[("--workspace", "ws2")]);
    assert_eq!(
        named(&workspace, &manager("runtime")),
        Err("deployment_conflict")
    );
    let same = flags(&[("--workspace", "ws1"), ("--agent", "agent-a")]);
    assert!(named(&same, &manager("runtime")).is_ok());
    let emptied = flags(&[("--agent", "")]);
    assert_eq!(
        named(&emptied, &manager("runtime")),
        Err("deployment_conflict")
    );
}

/// A manager variable never stands without `MANDATE_PROCESS`: the flags are not quietly preferred
/// over it, and the variables alone name nothing.
#[test]
fn a_manager_variable_without_its_process_is_refused() {
    let given = flags(&[("--workspace", "ws1"), ("--agent", "agent-a")]);
    for stray in ["MANDATE_WORKSPACE", "MANDATE_SUBJECT"] {
        let var = flags(&[(stray, "other")]);
        assert_eq!(named(&given, &var), Err("deployment_conflict"), "{stray}");
        assert_eq!(named(&none(), &var), Err("deployment_conflict"), "{stray}");
    }
}

/// The in-memory journal is refused on a pod the manager started, whatever the process it names,
/// and allowed anywhere else; without `--fixture` the manager's pod keeps its journal (the
/// executor's guard, #1754, applied to the runtime).
#[test]
fn the_fixture_journal_is_refused_on_a_managers_pod() {
    for process in ["runtime", "executor", ""] {
        assert_eq!(
            fixture_permitted(true, &manager(process)),
            Err("fixture_under_manager"),
            "{process:?}"
        );
    }
    assert_eq!(fixture_permitted(true, &none()), Ok(()));
    assert_eq!(fixture_permitted(false, &manager("runtime")), Ok(()));
    assert_eq!(fixture_permitted(false, &none()), Ok(()));
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// DEC-2397 item 6: `--gateway <host:port>` beside `--research` names the model-gateway service;
/// without the flag the runtime keeps its in-process recorded fake.
#[test]
#[ignore = "pending E21-62"]
fn the_gateway_flag_names_the_service_beside_research() {
    assert_eq!(
        launch::gateway(&words("run --research /b --gateway 127.0.0.1:8080"), true),
        Ok(Some("127.0.0.1:8080".parse().unwrap()))
    );
    assert_eq!(launch::gateway(&words("run --research /b"), true), Ok(None));
    assert_eq!(launch::gateway(&words("run"), false), Ok(None));
}

/// DEC-2397 item 6: the flag written `--gateway=`, with no value, with another flag in its place,
/// with a value that names no address, or without `--research`, refuses the start.
#[test]
#[ignore = "pending E21-62"]
fn a_gateway_flag_that_names_no_service_refuses_the_start() {
    for (text, research) in [
        ("run --research /b --gateway=127.0.0.1:8080", true),
        ("run --research /b --gateway", true),
        ("run --research /b --gateway --probe /p", true),
        ("run --research /b --gateway nowhere", true),
        ("run --gateway 127.0.0.1:8080", false),
        ("run --gateway=127.0.0.1:8080", false),
    ] {
        assert_eq!(
            launch::gateway(&words(text), research),
            Err("gateway_address_invalid"),
            "{text}"
        );
    }
}
