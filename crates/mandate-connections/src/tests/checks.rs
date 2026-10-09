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

use ConnectionState::{Degraded, Suspended};

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
    assert_eq!(report.refusal(), None);
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
    assert_eq!(mcp.refusal(), None);
}

#[test]
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
fn nothing_that_can_move_funds_out_is_accepted() {
    for permissions in [
        &["trade", "withdraw"][..],
        &["transfer", "trade"],
        &["deposit"],
        &["trade", "funding"],
        &["trade", "withdrawal"],
        &["trade", "Withdraw"],
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
    for tool in [
        "transfer_funds",
        "withdraw_crypto",
        "initiate_deposit",
        "create_ach_transfer",
        "send_crypto",
        "wire_out",
    ] {
        let mut input = robinhood();
        input.granted = Granted::Tools(set(&["get_accounts", "place_equity_order", tool]));
        assert_eq!(
            outcome(&run(&input).unwrap(), Check::Scope),
            Some(Outcome::Failed(Reason::FundMovement)),
            "an MCP server offering {tool} is refused (CN-2)"
        );
    }
    let mut oauth = alpaca();
    oauth.granted = Granted::OAuth(GrantedScopes(set(&["data", "trading", "transfer"])));
    assert_eq!(
        outcome(&run(&oauth).unwrap(), Check::Scope),
        Some(Outcome::Failed(Reason::FundMovement)),
        "a fund-movement scope is named as one, not only as a mismatch (CN-2)"
    );
}

/// Fund movement is judged by whole tokens (DEC-839 item 3, shared by check 1 under DEC-676 item
/// 1): a name is split at every character that is not a letter or digit, at each lower-to-upper
/// and digit-to-upper change, and before the last capital of a run of capitals that a lowercase
/// letter follows (`ACHDebit` is `ach`, `debit`); lowercased; and refused when a token is in
/// DEC-839's set. A name with any character that is not ASCII is refused whatever its tokens, so a
/// homoglyph cannot spell a fund word past the rule. A word that only contains one, such as
/// `fundamentals`, is another word. The rule is the same for every kind of grant: a key
/// permission, an OAuth scope, and an MCP tool.
#[test]
fn fund_movement_is_judged_by_whole_tokens() {
    let kraken_key = |name: &str| CheckInput {
        broker: Broker::KrakenDerivativesUs,
        auth_kind: AuthKind::ApiKey,
        granted: Granted::KeyPermissions(Some(set(&["trade", name]))),
        ..alpaca()
    };
    let alpaca_scope = |name: &str| CheckInput {
        granted: Granted::OAuth(GrantedScopes(set(&["data", "trading", name]))),
        ..alpaca()
    };
    let robinhood_tool = |name: &str| CheckInput {
        granted: Granted::Tools(set(&["get_accounts", "place_equity_order", name])),
        ..robinhood()
    };
    type Grant<'a> = &'a dyn Fn(&str) -> CheckInput;
    let kinds: [(&str, Grant, Outcome); 3] = [
        ("a key permission", &kraken_key, Outcome::Passed),
        (
            "an OAuth scope",
            &alpaca_scope,
            Outcome::Failed(Reason::ScopeMismatch),
        ),
        ("an MCP tool", &robinhood_tool, Outcome::Passed),
    ];
    for (kind, grant, other_word) in kinds {
        for name in FUND_MOVEMENT_NAMES.iter().chain(&FUND_MOVEMENT_TOKENS) {
            assert_eq!(
                outcome(&run(&grant(name)).unwrap(), Check::Scope),
                Some(Outcome::Failed(Reason::FundMovement)),
                "{name} as {kind} has a fund-movement token"
            );
        }
        for name in OTHER_WORDS {
            assert_eq!(
                outcome(&run(&grant(name)).unwrap(), Check::Scope),
                Some(other_word),
                "{name} as {kind} has no fund-movement token"
            );
        }
    }
}

/// Names with a fund-movement token, one per way a name is split, and a homoglyph.
const FUND_MOVEMENT_NAMES: [&str; 15] = [
    "ach_debit",
    "sendMoney",
    "initiate-ach-transfer",
    "get_transfers",
    "link_ach",
    "createAchRelationship",
    "request_payout",
    "disburse_now",
    "get_deposits",
    "add_funds",
    "ACHDebit",
    "getACHStatus",
    "HTTPSend",
    "v2Transfer",
    "w\u{456}re",
];

/// Every token of DEC-839 item 3's set, as a bare name.
const FUND_MOVEMENT_TOKENS: [&str; 15] = [
    "transfer",
    "transfers",
    "withdraw",
    "withdrawal",
    "withdrawals",
    "wire",
    "ach",
    "send",
    "payout",
    "disburse",
    "deposit",
    "deposits",
    "fund",
    "funds",
    "funding",
];

/// Names that contain a fund word only inside another word: as a scope each is still not
/// `data` or `trading`, so it is `scope_mismatch`, never `fund_movement`.
const OTHER_WORDS: [&str; 6] = [
    "get_fundamentals",
    "refund_status",
    "wireless",
    "resend",
    "teacher",
    "URLParser",
];

/// A pin is made at the first connect only; at every later occasion a missing pin fails closed as
/// drift (DEC-676 item 2, AGENTS.md rule 3), never as a pass.
#[test]
fn a_later_contract_check_without_a_pin_fails_closed() {
    for occasion in [
        Occasion::Reconnect,
        Occasion::Reauthorize,
        Occasion::ExecutorStart,
        Occasion::Daily,
    ] {
        let mut unpinned = robinhood();
        unpinned.occasion = occasion;
        unpinned.pinned_contract = None;
        assert_eq!(
            outcome(&run(&unpinned).unwrap(), Check::Contract),
            Some(Outcome::Failed(Reason::ContractDrift)),
            "{occasion:?}"
        );
    }
    let first = robinhood();
    assert_eq!(first.pinned_contract, None);
    assert_eq!(
        outcome(&run(&first).unwrap(), Check::Contract),
        Some(Outcome::Passed),
        "the first connect pins"
    );
}

/// What the broker reported must be the kind the credential is: an API key's permissions, OAuth
/// scopes, or MCP tools. Any other pairing fails closed as `scope_mismatch` (DEC-676 item 3).
#[test]
fn a_grant_of_another_kind_than_the_credential_is_refused() {
    let key = Granted::KeyPermissions(Some(set(&["trade"])));
    let scopes = Granted::OAuth(GrantedScopes(set(&["data", "trading"])));
    let tools = Granted::Tools(set(&["get_accounts", "place_equity_order"]));
    let kraken = CheckInput {
        broker: Broker::KrakenDerivativesUs,
        auth_kind: AuthKind::ApiKey,
        granted: key.clone(),
        ..alpaca()
    };
    let alpaca_key = CheckInput {
        auth_kind: AuthKind::ApiKey,
        granted: key.clone(),
        ..alpaca()
    };
    let cases: [(CheckInput, Granted); 9] = [
        (robinhood(), key.clone()),
        (robinhood(), Granted::KeyPermissions(None)),
        (robinhood(), scopes.clone()),
        (alpaca(), key.clone()),
        (alpaca(), tools.clone()),
        (alpaca_key.clone(), scopes.clone()),
        (alpaca_key, tools.clone()),
        (kraken.clone(), scopes),
        (kraken, tools),
    ];
    for (base, granted) in cases {
        let input = CheckInput {
            granted: granted.clone(),
            ..base
        };
        assert_eq!(
            outcome(&run(&input).unwrap(), Check::Scope),
            Some(Outcome::Failed(Reason::ScopeMismatch)),
            "{:?} given {granted:?}",
            input.auth_kind
        );
    }
}

#[test]
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
    let kraken_demo = run(&key(Broker::KrakenDerivativesUs, Environment::Paper)).unwrap();
    assert_eq!(
        outcome(&kraken_demo, Check::Scope),
        Some(Outcome::Passed),
        "a demo key too (DEC-441 item 4)"
    );
    assert_eq!(
        kraken_demo.refusal(),
        None,
        "and it is recorded, not refused"
    );
    for broker in [Broker::Alpaca, Broker::KrakenDerivativesUs] {
        assert_eq!(
            outcome(&run(&key(broker, Environment::Live)).unwrap(), Check::Scope),
            Some(Outcome::Failed(Reason::PermissionsUnreadable)),
            "{broker:?}"
        );
    }
}

/// A key whose permissions were read and are empty (DEC-685): a live one cannot show fund
/// movement absent, so it is refused as `permissions_unreadable` (CN-2, DEC-441 item 4); a paper
/// one cannot move money and still passes. A live key whose read permissions are not empty and
/// show no fund movement passes.
#[test]
fn a_live_key_that_reports_no_permissions_is_refused() {
    let empty_key = |environment| CheckInput {
        broker: Broker::KrakenDerivativesUs,
        environment,
        auth_kind: AuthKind::ApiKey,
        granted: Granted::KeyPermissions(Some(BTreeSet::new())),
        ..alpaca()
    };
    assert_eq!(
        outcome(&run(&empty_key(Environment::Live)).unwrap(), Check::Scope),
        Some(Outcome::Failed(Reason::PermissionsUnreadable)),
        "a live key that reports no permissions cannot show fund movement absent (DEC-685)"
    );
    assert_eq!(
        outcome(&run(&empty_key(Environment::Paper)).unwrap(), Check::Scope),
        Some(Outcome::Passed),
        "a paper key that reports no permissions is recorded, not refused (DEC-685)"
    );
    let trade_only = CheckInput {
        granted: Granted::KeyPermissions(Some(BTreeSet::from(["trade".to_owned()]))),
        ..empty_key(Environment::Live)
    };
    assert_eq!(
        outcome(&run(&trade_only).unwrap(), Check::Scope),
        Some(Outcome::Passed),
        "a live key whose read permissions show no fund movement passes check 1"
    );
}

#[test]
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
    let key = |broker, environment| CheckInput {
        broker,
        environment,
        auth_kind: AuthKind::ApiKey,
        granted: Granted::KeyPermissions(Some(set(&["trade"]))),
        ..alpaca()
    };
    for (broker, environment) in [
        (Broker::Alpaca, Environment::Paper),
        (Broker::Alpaca, Environment::Live),
        (Broker::KrakenDerivativesUs, Environment::Paper),
        (Broker::KrakenDerivativesUs, Environment::Live),
    ] {
        assert_eq!(
            outcome(&run(&key(broker, environment)).unwrap(), Check::Environment),
            Some(Outcome::Passed),
            "a key is issued for one environment's host only ({broker:?} {environment:?}); a live \
             Alpaca key is refused by the record (DEC-441 item 3) and check 1, not here"
        );
    }
}

#[test]
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
    let not_dedicated = run(&shared).unwrap();
    assert_eq!(
        outcome(&not_dedicated, Check::Account),
        Some(Outcome::Failed(Reason::NotDedicated))
    );
    assert_eq!(
        not_dedicated.account_pii_ref,
        Some(pii()),
        "the account was read, so its reference is recorded (journal rule 62)"
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
fn the_refusal_is_the_first_failed_check_in_section_8_1_order() {
    let mut everything = robinhood();
    everything.environment = Environment::Paper;
    everything.granted = Granted::Tools(set(&["get_accounts", "transfer_funds"]));
    everything.account = AccountRead::Unreadable;
    everything.contract = None;
    assert_eq!(
        run(&everything).unwrap().refusal(),
        Some((Check::Scope, Reason::FundMovement))
    );
    let mut later = robinhood();
    later.account = AccountRead::Unreadable;
    later.contract = None;
    assert_eq!(
        run(&later).unwrap().refusal(),
        Some((Check::Account, Reason::AccountUnreadable)),
        "check 3 before check 7"
    );
}

#[test]
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
fn a_later_failure_suspends_and_drift_degrades() {
    for occasion in [Occasion::ExecutorStart, Occasion::Daily] {
        let mut failed = alpaca();
        failed.occasion = occasion;
        failed.account = AccountRead::Unreadable;
        assert_eq!(
            run(&failed).unwrap().later_state(),
            Some(Suspended),
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
            Some(Degraded),
            "{occasion:?}"
        );
        let mut missing_and_drifted = drifted.clone();
        missing_and_drifted.contract = Some(ContractSeen {
            allowlisted_tools_present: false,
            hash: DRIFTED.to_owned(),
        });
        let report = run(&missing_and_drifted).unwrap();
        assert_eq!(
            outcome(&report, Check::Contract),
            Some(Outcome::Failed(Reason::ToolsMissing)),
            "a missing tool is reported over drift ({occasion:?})"
        );
        assert_eq!(
            report.later_state(),
            Some(Suspended),
            "DEC-674 ({occasion:?})"
        );
        let mut clean = alpaca();
        clean.occasion = occasion;
        assert_eq!(run(&clean).unwrap().later_state(), None);
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
            None,
            "a refusal, not a state change ({occasion:?})"
        );
    }
}

/// A report as [`run`] would make it, for the methods that only read one.
fn report(occasion: Occasion, failed: &[(Check, Reason)]) -> CheckReport {
    let results = [
        Check::Account,
        Check::Contract,
        Check::Environment,
        Check::Scope,
    ]
    .into_iter()
    .map(|check| {
        let reason = failed.iter().find(|(c, _)| *c == check).map(|(_, r)| *r);
        (check, reason.map_or(Outcome::Passed, Outcome::Failed))
    })
    .collect();
    CheckReport {
        connection_id: ConnectionId("conn_r".to_owned()),
        occasion,
        results,
        account_pii_ref: Some(pii()),
    }
}

/// The checks a report failed, with why.
type Failures<'a> = &'a [(Check, Reason)];

/// One failure of each check, in §8.1's order.
const ONE_EACH: [(Check, Reason); 4] = [
    (Check::Scope, Reason::FundMovement),
    (Check::Environment, Reason::WrongEnvironment),
    (Check::Account, Reason::NotDedicated),
    (Check::Contract, Reason::ContractDrift),
];

#[test]
fn each_check_has_its_journal_code() {
    for (check, code) in [
        (Check::Account, "account"),
        (Check::Contract, "contract"),
        (Check::Environment, "environment"),
        (Check::Scope, "scope"),
    ] {
        assert_eq!(check.code(), code);
    }
}

/// Every suffix of §8.1's order: the refusal is the earliest failed check, whatever the report's
/// own order (the record's, alphabetical).
#[test]
fn the_refusal_reads_any_report_in_section_8_1_order() {
    assert_eq!(report(Occasion::Connect, &[]).refusal(), None);
    for first in 0..ONE_EACH.len() {
        let failed = &ONE_EACH[first..];
        assert_eq!(
            report(Occasion::Connect, failed).refusal(),
            Some(ONE_EACH[first]),
            "{failed:?}"
        );
    }
}

#[test]
fn no_failed_check_ever_reaches_the_vault_write() {
    for failure in ONE_EACH {
        let stores = Cell::new(0);
        let store = || {
            stores.set(stores.get() + 1);
            Ok(())
        };
        assert_eq!(
            report(Occasion::Connect, &[failure]).then_store(store),
            Err(ConnectError::CheckRefused),
            "{failure:?}"
        );
        assert_eq!(stores.get(), 0, "no vault write after {failure:?}");
    }
    let stores = Cell::new(0);
    let store = || {
        stores.set(stores.get() + 1);
        Ok(())
    };
    assert_eq!(report(Occasion::Connect, &[]).then_store(store), Ok(()));
    assert_eq!(stores.get(), 1);
    assert_eq!(
        report(Occasion::Connect, &[]).then_store(|| Err(ConnectError::VaultUnavailable)),
        Err(ConnectError::VaultUnavailable),
        "the vault's own error is returned, never turned into success"
    );
}

/// Later occasions, against a table written out here: any scope, environment, or account failure
/// suspends, alone or with contract drift (journal rule 60: never degraded), and so does a missing
/// tool (DEC-674); contract drift alone degrades; a connect-time occasion never changes state.
#[test]
fn a_later_failure_suspends_unless_only_the_contract_drifted() {
    let drift = (Check::Contract, Reason::ContractDrift);
    let cases: [(Failures, Option<ConnectionState>); 10] = [
        (&[], None),
        (&[(Check::Scope, Reason::ScopeMismatch)], Some(Suspended)),
        (
            &[(Check::Environment, Reason::ReachesBoth)],
            Some(Suspended),
        ),
        (
            &[(Check::Account, Reason::AccountUnreadable)],
            Some(Suspended),
        ),
        (&[(Check::Account, Reason::NotDedicated)], Some(Suspended)),
        (&[drift], Some(Degraded)),
        (&[(Check::Contract, Reason::ToolsMissing)], Some(Suspended)),
        (
            &[(Check::Scope, Reason::FundMovement), drift],
            Some(Suspended),
        ),
        (
            &[(Check::Account, Reason::NotDedicated), drift],
            Some(Suspended),
        ),
        (
            &[
                (Check::Environment, Reason::WrongEnvironment),
                (Check::Contract, Reason::ToolsMissing),
            ],
            Some(Suspended),
        ),
    ];
    for (failed, want) in cases {
        for occasion in [Occasion::ExecutorStart, Occasion::Daily] {
            assert_eq!(
                report(occasion, failed).later_state(),
                want,
                "{occasion:?} {failed:?}"
            );
        }
        for occasion in [
            Occasion::Connect,
            Occasion::Reconnect,
            Occasion::Reauthorize,
        ] {
            assert_eq!(
                report(occasion, failed).later_state(),
                None,
                "{occasion:?} {failed:?}"
            );
        }
    }
}
