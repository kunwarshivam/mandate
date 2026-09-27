//! Fixtures shared by the hand tests and the properties: one equity request that every check
//! passes, and the context, response, and current state that admit and act on it. Each test
//! changes exactly the field its title names.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the fixtures"
)]

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::num::NonZeroU8;

use mandate_approval::{
    ActorKind, AdmissionContext, ApprovalRef, AskablePurpose, AssertionId, AssetClass, BoundAction,
    Classification, ContentHash, Current, DryRun, Environment, EvidenceAuthor, EvidenceRef,
    ModeNow, OpaqueUser, ReferenceMark, Request, RequestContent, Response, RiskClock, RiskField,
    RiskFigure, StepUp, StepUpMethod, Verdict,
};
use mandate_canon::{DecStr, Digest};
use mandate_num::{Price, Qty, Signed};

/// 2026-09-21T14:00:00Z, a Monday inside the regular session.
pub const T0: i64 = 1_789_999_200;
pub const DEADLINE: i64 = T0 + 300;
pub const OWNER: &str = "user-owner";
pub const SECOND: &str = "user-second";
pub const AUTHOR: &str = "user-author";
pub const REQUEST_ID: &str = "01J9ZQ4Y8N6K3V5T2R1M0P7XWA";

/// Unwraps an entry point's answer, panicking with the stub's own report while it is a stub.
pub fn answer<T, E: Debug>(what: &str, result: Result<T, E>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not {e:?}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap()
}

pub fn user(id: &str) -> OpaqueUser {
    OpaqueUser(id.to_owned())
}

pub fn bound() -> BoundAction {
    BoundAction {
        instrument: "asset-equity-1".to_owned(),
        asset_class: AssetClass::UsEquity,
        qty: Qty::parse("10").unwrap(),
        limit: price("187.25"),
        purpose: AskablePurpose::Open,
        mandate_version: "v3".to_owned(),
        decided_by: "rule:large_order".to_owned(),
        combined_score: Signed::parse("0.62").unwrap(),
        reference_mark: Some(ReferenceMark {
            price: price("187"),
            seq: 41,
        }),
        approvers_required: NonZeroU8::MIN,
        independent_required: false,
    }
}

/// The six §6.3 figures of the fixture's order, each with the mandate's own cap, every value and cap
/// distinct so a relabelled, zeroed, or dropped figure shows: the order value is 187.25 × 10.
pub const RISK_IMPACT: [(RiskField, &str, &str); 6] = [
    (RiskField::OrderUsd, "1872.5", "5000"),
    (RiskField::PositionUsdAfter, "2872.5", "10000"),
    (RiskField::GrossUsdAfter, "31872.5", "60000"),
    (RiskField::BoughtTodayUsd, "4872.5", "20000"),
    (RiskField::Drawdown, "0.031", "0.15"),
    (RiskField::DailyPnlFraction, "-0.004", "-0.03"),
];

pub fn content() -> RequestContent {
    RequestContent {
        bound: bound(),
        evidence: vec![
            EvidenceRef {
                event_id: "01J9ZQ4Y8N6K3V5T2R1M0P7XWB".to_owned(),
                artifact: None,
                author: EvidenceAuthor::OwnerSelected,
            },
            EvidenceRef {
                event_id: "01J9ZQ4Y8N6K3V5T2R1M0P7XWC".to_owned(),
                artifact: Some(Digest::of(b"thesis")),
                author: EvidenceAuthor::Platform,
            },
        ],
        risk_impact: RISK_IMPACT
            .iter()
            .map(|(field, value, cap)| RiskFigure {
                field: *field,
                value: DecStr::parse(value).unwrap(),
                cap: Some(DecStr::parse(cap).unwrap()),
            })
            .collect(),
        deadline: RiskClock(DEADLINE),
    }
}

/// A stand-in hash: admission compares hashes and never computes one.
pub fn hash(tag: &str) -> ContentHash {
    ContentHash(Digest::of(tag.as_bytes()))
}

pub fn request() -> Request {
    Request {
        id: ApprovalRef::of_requested_event(REQUEST_ID),
        content: content(),
        content_hash: hash("request"),
        delivered: true,
        grants: BTreeSet::new(),
    }
}

pub fn ctx() -> AdmissionContext {
    AdmissionContext {
        folded_clock: RiskClock(T0 + 10),
        approvers: [user(OWNER), user(SECOND)].into_iter().collect(),
        author: user(AUTHOR),
        environment: Environment::Paper,
        used_assertions: BTreeSet::new(),
    }
}

pub fn step_up(assertion: &str, authenticated_at: i64) -> StepUp {
    StepUp {
        assertion: AssertionId(assertion.to_owned()),
        authenticated_at: RiskClock(authenticated_at),
        method: StepUpMethod::CliConfirm,
    }
}

/// A timely grant from the owner with fresh evidence, repeating the request's hash.
pub fn grant() -> Response {
    Response {
        source: "01J9ZQ4Y8N6K3V5T2R1M0P7XWD".to_owned(),
        approval: ApprovalRef::of_requested_event(REQUEST_ID),
        actor_kind: ActorKind::User,
        responder: user(OWNER),
        verdict: Verdict::Approve(Some(step_up("assertion-1", T0 + 5))),
        content_hash: hash("request"),
        submitted_at: RiskClock(T0 + 8),
    }
}

pub fn skip() -> Response {
    Response {
        verdict: Verdict::Skip,
        ..grant()
    }
}

/// The current state in which the fixture's grant acts.
pub fn current() -> Current {
    Current {
        mandate_version: "v3".to_owned(),
        mode: ModeNow::Normal,
        instrument_restricted: false,
        in_working_universe: true,
        classification: Classification::Ask {
            decided_by: "rule:large_order".to_owned(),
        },
        dry_run: DryRun::Allow,
        mark_now: Some(price("187")),
    }
}
