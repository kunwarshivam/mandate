//! DEC-923's scoped live `cli_confirm` (DEC-529 item 3, DEC-927): the scope judge's one admission
//! and each of its refusals, admission check 6 on a `live` stream with and without the scope, and a
//! property against an oracle written from DEC-923's text alone.

mod common;

use std::collections::BTreeSet;

use common::{OWNER, SECOND, T0, answer, ctx, grant, price, request, step_up, user};
use mandate_approval::{
    Admission, AdmissionContext, AssetClass, Environment, LIVE_CLI_CONFIRM_MAX_ORDER_USD,
    LiveAdmission, LiveBroker, LiveGrant, LiveRefusal, LiveScope, OpaqueUser, Refusal, Request,
    Verdict, admit, live_cli_confirm,
};
use mandate_num::{Qty, Usd};
use proptest::prelude::*;

const ETF: &str = "asset-etf-1";

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

/// DEC-529's deployment: Robinhood, a 100 USD cap, one pinned ETF, the owner as workspace owner.
fn scope() -> LiveScope {
    LiveScope {
        broker: LiveBroker::Robinhood,
        max_order_usd: usd("100"),
        pinned_instruments: BTreeSet::from([ETF.to_owned()]),
        workspace_owner: user(OWNER),
    }
}

fn judged(scope: Option<&LiveScope>, grant: &LiveGrant<'_>) -> LiveAdmission {
    answer("live_cli_confirm", live_cli_confirm(scope, grant))
}

fn one_share(owner: &OpaqueUser) -> LiveGrant<'_> {
    LiveGrant {
        instrument: ETF,
        asset_class: AssetClass::UsEquity,
        order_usd: usd("89.5"),
        approver: owner,
    }
}

#[test]
fn the_cap_is_dec_529s_hundred_dollars() {
    assert_eq!(usd(LIVE_CLI_CONFIRM_MAX_ORDER_USD), usd("100"));
}

/// DEC-923 item 1: the one admission.
#[test]
fn a_robinhood_deployment_within_dec_529_admits_the_owners_grant() {
    let owner = user(OWNER);
    assert_eq!(
        judged(Some(&scope()), &one_share(&owner)),
        LiveAdmission::Admitted
    );
    let at_the_cap = LiveGrant {
        order_usd: usd("100"),
        ..one_share(&owner)
    };
    assert_eq!(
        judged(Some(&scope()), &at_the_cap),
        LiveAdmission::Admitted,
        "100 USD exactly is within DEC-529's cap"
    );
}

/// DEC-923 items 1 and 3: each condition, broken alone, refuses with its own reason.
#[test]
fn every_other_live_deployment_keeps_refusing() {
    let owner = user(OWNER);
    let other = user(SECOND);
    let base = scope();
    let grant = one_share(&owner);
    let refused = |s: Option<&LiveScope>, g: &LiveGrant<'_>, why: LiveRefusal| {
        assert_eq!(judged(s, g), LiveAdmission::Refused(why), "{why:?}");
    };
    refused(None, &grant, LiveRefusal::NoScope);
    refused(
        Some(&LiveScope {
            broker: LiveBroker::Other,
            ..base.clone()
        }),
        &grant,
        LiveRefusal::NotRobinhood,
    );
    refused(
        Some(&LiveScope {
            max_order_usd: usd("100.01"),
            ..base.clone()
        }),
        &grant,
        LiveRefusal::MandateCapAboveLimit,
    );
    refused(
        Some(&LiveScope {
            pinned_instruments: BTreeSet::new(),
            ..base.clone()
        }),
        &grant,
        LiveRefusal::NotOneInstrument,
    );
    refused(
        Some(&LiveScope {
            pinned_instruments: BTreeSet::from([ETF.to_owned(), "asset-etf-2".to_owned()]),
            ..base.clone()
        }),
        &grant,
        LiveRefusal::NotOneInstrument,
    );
    refused(
        Some(&base),
        &LiveGrant {
            instrument: "asset-etf-2",
            ..grant
        },
        LiveRefusal::OtherInstrument,
    );
    refused(
        Some(&base),
        &LiveGrant {
            asset_class: AssetClass::Crypto,
            ..grant
        },
        LiveRefusal::NotEquity,
    );
    refused(
        Some(&base),
        &LiveGrant {
            order_usd: usd("100.000000001"),
            ..grant
        },
        LiveRefusal::OrderAboveLimit,
    );
    refused(
        Some(&base),
        &LiveGrant {
            approver: &other,
            ..grant
        },
        LiveRefusal::NotOwner,
    );
}

#[test]
fn each_refusal_has_its_own_code() {
    let all = [
        LiveRefusal::NoScope,
        LiveRefusal::NotRobinhood,
        LiveRefusal::MandateCapAboveLimit,
        LiveRefusal::NotOneInstrument,
        LiveRefusal::OtherInstrument,
        LiveRefusal::NotEquity,
        LiveRefusal::OrderAboveLimit,
        LiveRefusal::NotOwner,
    ];
    let codes: BTreeSet<&str> = all.iter().map(|r| r.code()).collect();
    assert_eq!(codes.len(), all.len());
    assert!(codes.iter().all(|c| c.starts_with("live_")));
    assert_eq!(LiveRefusal::NotOwner.code(), "live_not_owner");
    assert_eq!(LiveRefusal::NotRobinhood.code(), "live_not_robinhood");
}

/// The fixture's request, its bound order one share of the ETF at 89.5 USD.
fn live_request() -> Request {
    let mut r = request();
    r.content.bound.instrument = ETF.to_owned();
    r.content.bound.qty = Qty::parse("1").unwrap();
    r.content.bound.limit = price("89.5");
    r
}

fn live_ctx(scope: Option<LiveScope>) -> AdmissionContext {
    AdmissionContext {
        environment: Environment::Live,
        live_cli_confirm: scope,
        ..ctx()
    }
}

fn admitted(request: &Request, ctx: &AdmissionContext) -> Admission {
    answer("admit", admit(Some(request), &grant(), ctx))
}

/// DEC-923 item 1 at admission: the owner's fresh `cli_confirm` grant on the scoped live deployment
/// is admitted; without the scope the same grant is refused for its method, as before.
#[test]
fn check_6_admits_cli_confirm_on_live_only_within_the_scope() {
    let request = live_request();
    assert_eq!(
        admitted(&request, &live_ctx(Some(scope()))),
        Admission::Admitted
    );
    assert_eq!(
        admitted(&request, &live_ctx(None)),
        Admission::Refused(Refusal::StepUpMethod)
    );
    assert_eq!(
        admitted(&request, &ctx()),
        Admission::Admitted,
        "paper is unchanged"
    );
}

/// DEC-923 item 3 at admission: each out-of-scope deployment or order refuses the method.
#[test]
fn check_6_refuses_cli_confirm_on_every_other_live_deployment() {
    let request = live_request();
    let refused = |r: &Request, s: LiveScope, what: &str| {
        assert_eq!(
            admitted(r, &live_ctx(Some(s))),
            Admission::Refused(Refusal::StepUpMethod),
            "{what}"
        );
    };
    refused(
        &request,
        LiveScope {
            broker: LiveBroker::Other,
            ..scope()
        },
        "another broker",
    );
    refused(
        &request,
        LiveScope {
            max_order_usd: usd("101"),
            ..scope()
        },
        "a mandate cap above 100 USD",
    );
    refused(
        &request,
        LiveScope {
            pinned_instruments: BTreeSet::from([ETF.to_owned(), "asset-etf-2".to_owned()]),
            ..scope()
        },
        "two pinned instruments",
    );
    refused(
        &request,
        LiveScope {
            workspace_owner: user(SECOND),
            ..scope()
        },
        "an approver who is not the workspace owner",
    );
    let mut costly = live_request();
    costly.content.bound.qty = Qty::parse("2").unwrap();
    refused(&costly, scope(), "an order above 100 USD");
    let mut other = live_request();
    other.content.bound.instrument = "asset-etf-2".to_owned();
    refused(&other, scope(), "another instrument");
    let mut crypto = live_request();
    crypto.content.bound.asset_class = AssetClass::Crypto;
    refused(&crypto, scope(), "crypto");
}

/// The scope never rescues evidence that fails for another reason, and never touches a skip.
#[test]
fn the_scope_admits_only_the_method() {
    let request = live_request();
    let ctx = live_ctx(Some(scope()));
    let stale = mandate_approval::Response {
        verdict: Verdict::Approve(Some(step_up("assertion-1", T0 - 400))),
        ..grant()
    };
    assert_eq!(
        answer("admit", admit(Some(&request), &stale, &ctx)),
        Admission::Refused(Refusal::StepUpStale)
    );
    let missing = mandate_approval::Response {
        verdict: Verdict::Approve(None),
        ..grant()
    };
    assert_eq!(
        answer("admit", admit(Some(&request), &missing, &ctx)),
        Admission::Refused(Refusal::StepUpMissing)
    );
    let mut used = ctx.clone();
    used.used_assertions
        .insert(mandate_approval::AssertionId("assertion-1".to_owned()));
    assert_eq!(
        answer("admit", admit(Some(&request), &grant(), &used)),
        Admission::Refused(Refusal::StepUpReused)
    );
    let skip = mandate_approval::Response {
        verdict: Verdict::Skip,
        ..grant()
    };
    assert_eq!(
        answer("admit", admit(Some(&request), &skip, &live_ctx(None))),
        Admission::Admitted,
        "a skip needs no step-up on live either (PX-7)"
    );
}

/// The oracle: DEC-923's conditions, read as text and cents on `i128`, sharing no code with the
/// crate.
fn oracle(
    robinhood: bool,
    cap_cents: i128,
    pinned: &[&str],
    instrument: &str,
    equity: bool,
    order_cents: i128,
    approver_is_owner: bool,
) -> bool {
    robinhood
        && cap_cents <= 10_000
        && pinned.len() == 1
        && pinned.first() == Some(&instrument)
        && equity
        && order_cents <= 10_000
        && approver_is_owner
}

/// Whole positive cents as the canonical decimal text `Usd` parses: no trailing fractional zeros.
fn cents(c: i128) -> Usd {
    let (whole, part) = (c / 100, c % 100);
    usd(&match (part, part % 10) {
        (0, _) => whole.to_string(),
        (_, 0) => format!("{whole}.{}", part / 10),
        _ => format!("{whole}.{part:02}"),
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over every combination of the seven conditions, values near the cap weighted in, the judge
    /// admits exactly when the oracle does, and through admission check 6 the grant is admitted
    /// exactly then and refused for its method otherwise.
    #[test]
    fn the_judge_admits_exactly_what_dec_923_allows(
        robinhood in prop::bool::weighted(0.8),
        cap_cents in prop_oneof![Just(10_000i128), Just(10_001i128), 1i128..20_000],
        pinned_shape in prop_oneof![3 => Just(1usize), 1 => 0usize..4],
        on_pinned in prop::bool::weighted(0.8),
        equity in prop::bool::weighted(0.8),
        order_cents in prop_oneof![Just(10_000i128), Just(10_001i128), 1i128..20_000],
        approver_is_owner in prop::bool::weighted(0.8),
    ) {
        let pinned: Vec<&str> = match pinned_shape {
            0 => vec![],
            1 => vec![ETF],
            2 => vec!["asset-etf-2"],
            _ => vec![ETF, "asset-etf-2"],
        };
        let instrument = if on_pinned { ETF } else { "asset-etf-3" };
        let owner = user(OWNER);
        let other = user(SECOND);
        let scope = LiveScope {
            broker: if robinhood { LiveBroker::Robinhood } else { LiveBroker::Other },
            max_order_usd: cents(cap_cents),
            pinned_instruments: pinned.iter().map(|p| (*p).to_owned()).collect(),
            workspace_owner: owner.clone(),
        };
        let class = if equity { AssetClass::UsEquity } else { AssetClass::Crypto };
        let grant = LiveGrant {
            instrument,
            asset_class: class,
            order_usd: cents(order_cents),
            approver: if approver_is_owner { &owner } else { &other },
        };
        let expected = oracle(
            robinhood, cap_cents, &pinned, instrument, equity, order_cents, approver_is_owner,
        );
        let verdict = live_cli_confirm(Some(&scope), &grant)
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        prop_assert_eq!(verdict == LiveAdmission::Admitted, expected);

        let mut request = live_request();
        request.content.bound.instrument = instrument.to_owned();
        request.content.bound.asset_class = class;
        request.content.bound.qty = Qty::parse("1").unwrap();
        request.content.bound.limit = mandate_num::Price::parse(&cents(order_cents).to_string()).unwrap();
        let context = live_ctx(Some(scope));
        let response = mandate_approval::Response {
            responder: if approver_is_owner { owner.clone() } else { other.clone() },
            ..common::grant()
        };
        let admitted = admit(Some(&request), &response, &context)
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        prop_assert_eq!(
            admitted,
            if expected { Admission::Admitted } else { Admission::Refused(Refusal::StepUpMethod) }
        );
    }
}
