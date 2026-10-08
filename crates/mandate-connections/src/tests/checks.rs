//! The permission checks refuse what they must, from what the executor already read, and never
//! let a refused credential be stored (connections spec §8.1; CN-2, CN-3, CN-10; E7-12).

use std::cell::Cell;
use std::collections::BTreeSet;

use crate::ConnectError;
use crate::checks::{
    AccountRead, Check, CheckInput, CheckReport, ContractSeen, Granted, Occasion, Outcome, Reason,
    run,
};
use crate::grant::GrantedScopes;
use crate::record::{AccountPiiRef, AuthKind, Broker, ConnectionId, ConnectionState, Environment};

const PINNED: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DRIFTED: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn pii() -> AccountPiiRef {
    AccountPiiRef("pii_acct_7Q2M".to_owned())
}

fn read() -> AccountRead {
    AccountRead::Read {
        pii_ref: pii(),
        dedicated: None,
    }
}

/// An Alpaca paper OAuth connect that passes every check.
fn alpaca() -> CheckInput {
    CheckInput {
        connection_id: ConnectionId("conn_a".to_owned()),
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        auth_kind: AuthKind::Oauth,
        occasion: Occasion::Connect,
        granted: Granted::OAuth(GrantedScopes(set(&["data", "trading"]))),
        account: read(),
        contract: None,
        pinned_contract: None,
    }
}

/// A Robinhood MCP connect that passes every check.
fn robinhood() -> CheckInput {
    CheckInput {
        broker: Broker::Robinhood,
        environment: Environment::Live,
        auth_kind: AuthKind::McpOauth,
        granted: Granted::Tools(set(&["get_accounts", "place_equity_order"])),
        account: AccountRead::Read {
            pii_ref: pii(),
            dedicated: Some(true),
        },
        contract: Some(ContractSeen {
            allowlisted_tools_present: true,
            hash: PINNED.to_owned(),
        }),
        ..alpaca()
    }
}

fn outcome(report: &CheckReport, check: Check) -> Option<Outcome> {
    report
        .results
        .iter()
        .find(|(c, _)| *c == check)
        .map(|(_, o)| *o)
}

#[test]
fn each_reason_has_its_journal_code() {
    for (reason, code) in [
        (Reason::ScopeMismatch, "scope_mismatch"),
        (Reason::FundMovement, "fund_movement"),
        (Reason::PermissionsUnreadable, "permissions_unreadable"),
        (Reason::WrongEnvironment, "wrong_environment"),
        (Reason::ReachesBoth, "reaches_both"),
        (Reason::AccountUnreadable, "account_unreadable"),
        (Reason::NotDedicated, "not_dedicated"),
        (Reason::ToolsMissing, "tools_missing"),
        (Reason::ContractDrift, "contract_drift"),
    ] {
        assert_eq!(reason.code(), code);
    }
}

#[test]
#[ignore = "pending E7-12"]
fn a_clean_connect_reports_every_check_passed() {
    let report = run(&alpaca()).unwrap();
    assert_eq!(
        report,
        CheckReport {
            connection_id: ConnectionId("conn_a".to_owned()),
            occasion: Occasion::Connect,
            results: vec![
                (Check::Account, Outcome::Passed),
                (Check::Environment, Outcome::Passed),
                (Check::Scope, Outcome::Passed),
            ],
            account_pii_ref: Some(pii()),
        },
        "results in the record's order; contract only for MCP"
    );
    assert_eq!(report.refusal(), Ok(None));
    let mcp = run(&robinhood()).unwrap();
    let checks: Vec<Check> = mcp.results.iter().map(|(c, _)| *c).collect();
    assert_eq!(
        checks,
        [
            Check::Account,
            Check::Contract,
            Check::Environment,
            Check::Scope
        ]
    );
    assert_eq!(mcp.refusal(), Ok(None));
}

#[test]
#[ignore = "pending E7-12"]
fn scopes_must_be_exactly_trading_and_data() {
    for granted in [
        &["trading"][..],
        &["data"],
        &[],
        &["data", "trading", "account:write"],
        &["data", "trading", "account:read"],
    ] {
        let mut input = alpaca();
        input.granted = Granted::OAuth(GrantedScopes(set(granted)));
        let report = run(&input).unwrap();
        assert_eq!(
            outcome(&report, Check::Scope),
            Some(Outcome::Failed(Reason::ScopeMismatch)),
            "{granted:?}"
        );
    }
}

#[test]
#[ignore = "pending E7-12"]
fn nothing_that_can_move_funds_out_is_accepted() {
    for permissions in [
        &["trade", "withdraw"][..],
        &["transfer", "trade"],
        &["deposit"],
    ] {
        let input = CheckInput {
            broker: Broker::KrakenDerivativesUs,
            auth_kind: AuthKind::ApiKey,
            granted: Granted::KeyPermissions(Some(set(permissions))),
            ..alpaca()
        };
        assert_eq!(
            outcome(&run(&input).unwrap(), Check::Scope),
            Some(Outcome::Failed(Reason::FundMovement)),
            "{permissions:?}"
        );
    }
    for tool in ["transfer_funds", "withdraw_crypto", "initiate_deposit"] {
        let mut input = robinhood();
        input.granted = Granted::Tools(set(&["get_accounts", "place_equity_order", tool]));
        assert_eq!(
            outcome(&run(&input).unwrap(), Check::Scope),
            Some(Outcome::Failed(Reason::FundMovement)),
            "an MCP server offering {tool} is refused (CN-2)"
        );
    }
}

#[test]
#[ignore = "pending E7-12"]
fn a_live_key_whose_permissions_cannot_be_read_is_refused() {
    let key = |broker, environment| CheckInput {
        broker,
        environment,
        auth_kind: AuthKind::ApiKey,
        granted: Granted::KeyPermissions(None),
        ..alpaca()
    };
    assert_eq!(
        outcome(
            &run(&key(Broker::Alpaca, Environment::Paper)).unwrap(),
            Check::Scope
        ),
        Some(Outcome::Passed),
        "a paper key is recorded and disclosed (DEC-441 item 4)"
    );
    for broker in [Broker::Alpaca, Broker::KrakenDerivativesUs] {
        assert_eq!(
            outcome(&run(&key(broker, Environment::Live)).unwrap(), Check::Scope),
            Some(Outcome::Failed(Reason::PermissionsUnreadable)),
            "{broker:?}"
        );
    }
}

#[test]
#[ignore = "pending E7-12"]
fn the_environment_is_judged_from_documentation_without_a_request() {
    let mut live_oauth = alpaca();
    live_oauth.environment = Environment::Live;
    live_oauth.account = AccountRead::Unreadable;
    assert_eq!(
        outcome(&run(&live_oauth).unwrap(), Check::Environment),
        Some(Outcome::Failed(Reason::ReachesBoth)),
        "an Alpaca grant is not shown to reach only live, so it is refused (DEC-441 item 21); \
         nothing was read, so nothing reached the other host"
    );
    assert_eq!(
        outcome(&run(&alpaca()).unwrap(), Check::Environment),
        Some(Outcome::Passed),
        "a paper grant requested with env=paper (DEC-821 item 1)"
    );
    let mut robinhood_paper = robinhood();
    robinhood_paper.environment = Environment::Paper;
    assert_eq!(
        outcome(&run(&robinhood_paper).unwrap(), Check::Environment),
        Some(Outcome::Failed(Reason::WrongEnvironment)),
        "Robinhood has no paper environment (DEC-124)"
    );
    assert_eq!(
        outcome(&run(&robinhood()).unwrap(), Check::Environment),
        Some(Outcome::Passed)
    );
}

#[test]
#[ignore = "pending E7-12"]
fn the_account_is_read_and_for_robinhood_dedicated() {
    let mut unreadable = alpaca();
    unreadable.account = AccountRead::Unreadable;
    let report = run(&unreadable).unwrap();
    assert_eq!(
        outcome(&report, Check::Account),
        Some(Outcome::Failed(Reason::AccountUnreadable))
    );
    assert_eq!(report.account_pii_ref, None);
    let mut shared = robinhood();
    shared.account = AccountRead::Read {
        pii_ref: pii(),
        dedicated: Some(false),
    };
    assert_eq!(
        outcome(&run(&shared).unwrap(), Check::Account),
        Some(Outcome::Failed(Reason::NotDedicated))
    );
    shared.account = AccountRead::Read {
        pii_ref: pii(),
        dedicated: None,
    };
    assert_eq!(
        outcome(&run(&shared).unwrap(), Check::Account),
        Some(Outcome::Failed(Reason::NotDedicated)),
        "an account not shown to be the dedicated one is not"
    );
}

#[test]
#[ignore = "pending E7-12"]
fn an_mcp_contract_is_pinned_then_held() {
    let mut pinned = robinhood();
    pinned.occasion = Occasion::Daily;
    pinned.pinned_contract = Some(PINNED.to_owned());
    assert_eq!(
        outcome(&run(&pinned).unwrap(), Check::Contract),
        Some(Outcome::Passed)
    );
    pinned.contract = Some(ContractSeen {
        allowlisted_tools_present: true,
        hash: DRIFTED.to_owned(),
    });
    assert_eq!(
        outcome(&run(&pinned).unwrap(), Check::Contract),
        Some(Outcome::Failed(Reason::ContractDrift))
    );
    for seen in [
        Some(ContractSeen {
            allowlisted_tools_present: false,
            hash: PINNED.to_owned(),
        }),
        None,
    ] {
        let mut missing = robinhood();
        missing.contract = seen.clone();
        assert_eq!(
            outcome(&run(&missing).unwrap(), Check::Contract),
            Some(Outcome::Failed(Reason::ToolsMissing)),
            "{seen:?}"
        );
    }
}

#[test]
#[ignore = "pending E7-12"]
fn the_refusal_is_the_first_failed_check_in_section_8_1_order() {
    let mut everything = robinhood();
    everything.environment = Environment::Paper;
    everything.granted = Granted::Tools(set(&["get_accounts", "transfer_funds"]));
    everything.account = AccountRead::Unreadable;
    everything.contract = None;
    assert_eq!(
        run(&everything).unwrap().refusal(),
        Ok(Some((Check::Scope, Reason::FundMovement)))
    );
    let mut later = robinhood();
    later.account = AccountRead::Unreadable;
    later.contract = None;
    assert_eq!(
        run(&later).unwrap().refusal(),
        Ok(Some((Check::Account, Reason::AccountUnreadable))),
        "check 3 before check 7"
    );
}

#[test]
#[ignore = "pending E7-12"]
fn a_refused_credential_is_never_stored() {
    let stores = Cell::new(0);
    let store = || {
        stores.set(stores.get() + 1);
        Ok(())
    };
    let mut refused = alpaca();
    refused.granted = Granted::OAuth(GrantedScopes(set(&["data", "trading", "account:write"])));
    assert_eq!(
        run(&refused).unwrap().then_store(store),
        Err(ConnectError::CheckRefused)
    );
    assert_eq!(stores.get(), 0, "no vault write after a refusal");
    assert_eq!(run(&alpaca()).unwrap().then_store(store), Ok(()));
    assert_eq!(stores.get(), 1);
}

#[test]
#[ignore = "pending E7-12"]
fn a_later_failure_suspends_and_drift_degrades() {
    for occasion in [Occasion::ExecutorStart, Occasion::Daily] {
        let mut failed = alpaca();
        failed.occasion = occasion;
        failed.account = AccountRead::Unreadable;
        assert_eq!(
            run(&failed).unwrap().later_state(),
            Ok(Some(ConnectionState::Suspended)),
            "{occasion:?}"
        );
        let mut drifted = robinhood();
        drifted.occasion = occasion;
        drifted.pinned_contract = Some(PINNED.to_owned());
        drifted.contract = Some(ContractSeen {
            allowlisted_tools_present: true,
            hash: DRIFTED.to_owned(),
        });
        assert_eq!(
            run(&drifted).unwrap().later_state(),
            Ok(Some(ConnectionState::Degraded)),
            "{occasion:?}"
        );
        let mut clean = alpaca();
        clean.occasion = occasion;
        assert_eq!(run(&clean).unwrap().later_state(), Ok(None));
    }
    for occasion in [
        Occasion::Connect,
        Occasion::Reconnect,
        Occasion::Reauthorize,
    ] {
        let mut refused = alpaca();
        refused.occasion = occasion;
        refused.account = AccountRead::Unreadable;
        assert_eq!(
            run(&refused).unwrap().later_state(),
            Ok(None),
            "a refusal, not a state change ({occasion:?})"
        );
    }
}
