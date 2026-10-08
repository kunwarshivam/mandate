//! Spec §3.2's catalogue, §4.2's text set, and NT-12: every kind maps to its class and text key,
//! and no text advises, urges, or counts.

mod common;

use common::answer;
use mandate_notify::{Class, NoticeKind, TextKey};

use Class::{Action, Info, Safety};
use TextKey::{AccountChanged, ApprovalNeeded, AttentionNeeded, BriefReady};

/// Spec §3.2, row by row in its order, typed from the table rather than from the crate.
#[rustfmt::skip]
const CATALOGUE: [(NoticeKind, &str, Class, Option<TextKey>); 32] = [
    (NoticeKind::ApprovalRequested, "approval_requested", Action, Some(ApprovalNeeded)),
    (NoticeKind::ApprovalReminder, "approval_reminder", Action, Some(ApprovalNeeded)),
    (NoticeKind::RiskLimit, "risk_limit", Safety, Some(AttentionNeeded)),
    (NoticeKind::KillSwitch, "kill_switch", Safety, Some(AttentionNeeded)),
    (NoticeKind::AgentHeld, "agent_held", Safety, Some(AttentionNeeded)),
    (NoticeKind::AccountRestriction, "account_restriction", Safety, Some(AttentionNeeded)),
    (NoticeKind::Protection, "protection", Safety, Some(AttentionNeeded)),
    (NoticeKind::ExitStalled, "exit_stalled", Safety, Some(AttentionNeeded)),
    (NoticeKind::Reconciliation, "reconciliation", Safety, Some(AttentionNeeded)),
    (NoticeKind::ExternalActivity, "external_activity", Safety, Some(AttentionNeeded)),
    (NoticeKind::AccountState, "account_state", Safety, Some(AttentionNeeded)),
    (NoticeKind::DataFeedDown, "data_feed_down", Safety, Some(AttentionNeeded)),
    (NoticeKind::IntegrityIncident, "integrity_incident", Safety, Some(AttentionNeeded)),
    (NoticeKind::CredentialAdded, "credential_added", Safety, Some(AccountChanged)),
    (NoticeKind::NewDevice, "new_device", Safety, Some(AccountChanged)),
    (NoticeKind::RecoveryUsed, "recovery_used", Safety, Some(AccountChanged)),
    (NoticeKind::RoleGranted, "role_granted", Safety, Some(AccountChanged)),
    (NoticeKind::MemberDeactivated, "member_deactivated", Safety, Some(AccountChanged)),
    (NoticeKind::Deprovisioned, "deprovisioned", Safety, Some(AccountChanged)),
    (NoticeKind::BreakGlass, "break_glass", Safety, Some(AccountChanged)),
    (NoticeKind::VersionRiskIncreasing, "version_risk_increasing", Safety, Some(AccountChanged)),
    (NoticeKind::DelegationAdded, "delegation_added", Safety, Some(AccountChanged)),
    (NoticeKind::ConnectionAdded, "connection_added", Safety, Some(AccountChanged)),
    (NoticeKind::WentLive, "went_live", Safety, Some(AccountChanged)),
    (NoticeKind::ClientConnected, "client_connected", Safety, Some(AccountChanged)),
    (NoticeKind::ChannelLost, "channel_lost", Safety, Some(AccountChanged)),
    (NoticeKind::DailyBrief, "daily_brief", Info, Some(BriefReady)),
    (NoticeKind::DelegationEnded, "delegation_ended", Info, None),
    (NoticeKind::ModelStatus, "model_status", Info, None),
    (NoticeKind::ResearchStatus, "research_status", Info, None),
    (NoticeKind::SpendCap, "spend_cap", Info, None),
    (NoticeKind::ApprovalClosed, "approval_closed", Info, None),
];

/// Spec §4.2's table.
#[rustfmt::skip]
const TEXTS: [(TextKey, &str, &str); 4] = [
    (ApprovalNeeded, "approval_needed", "An agent in your workspace needs your approval"),
    (AttentionNeeded, "attention_needed", "Your workspace has a new alert"),
    (AccountChanged, "account_changed", "There was a change to your account or workspace access"),
    (BriefReady, "brief_ready", "Your daily brief is ready"),
];

#[test]
fn the_crate_lists_every_kind_once_in_the_catalogues_order() {
    let listed: Vec<NoticeKind> = CATALOGUE.iter().map(|row| row.0).collect();
    assert_eq!(NoticeKind::ALL.to_vec(), listed);
    assert_eq!(
        TextKey::ALL.to_vec(),
        TEXTS.iter().map(|row| row.0).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "pending E8-9"]
fn every_kind_maps_to_its_key_class_and_text_key() {
    for (kind, key, class, text) in CATALOGUE {
        assert_eq!(answer("key", kind.key()), key);
        assert_eq!(answer("class", kind.class()), class, "{key}");
        assert_eq!(answer("text_key", kind.text_key()), text, "{key}");
    }
}

#[test]
#[ignore = "pending E8-9"]
fn every_class_is_journaled_by_its_key() {
    let keys = [Action, Safety, Info].map(|c| answer("class key", c.key()));
    assert_eq!(keys, ["action", "safety", "info"]);
}

#[test]
#[ignore = "pending E8-9"]
fn every_text_key_renders_its_fixed_english_text() {
    for (text, key, english) in TEXTS {
        assert_eq!(answer("key", text.key()), key);
        assert_eq!(answer("text", text.text()), english);
    }
}

/// NT-12: first the advice-wording list of `the_content_never_carries_advice_wording` in
/// `crates/mandate-approval/tests/content.rs`, copied word for word: both crates sit at layer 1,
/// so neither may depend on the other, and a shared list would need a crate of its own. Then the
/// urgency, outcome, and trade words NT-12 adds, as whole words, and no figure at all. A word added
/// to either list belongs in both.
#[test]
#[ignore = "pending E8-9"]
fn no_text_advises_urges_or_counts() {
    let advice = "recommend estimate target scorecard expected suggest".split(' ');
    let forbidden =
        "now urgent urgently immediately asap hurry act quick profit loss gain gains buy \
                     sell missed opportunity"
            .split_whitespace();
    for text in TextKey::ALL {
        let english = answer("text", text.text()).to_lowercase();
        for word in advice.clone() {
            assert!(!english.contains(word), "{english:?} says {word:?}");
        }
        let words: Vec<&str> = english.split(|c: char| !c.is_ascii_alphabetic()).collect();
        for word in forbidden.clone() {
            assert!(!words.contains(&word), "{english:?} says {word:?}");
        }
        assert!(
            english.chars().all(|c| c.is_ascii_lowercase() || c == ' '),
            "{english:?} carries a figure or a mark"
        );
    }
}
