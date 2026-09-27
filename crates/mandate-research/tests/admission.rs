//! Hand-calculated admission cases: one per §8.5 check, plus the rules family N does not reach.
//!
//! Each case starts from [`Scenario::admitting`] (MC-N01, where every check passes) and breaks
//! exactly one thing, so a failure names the check that moved. Where the spec's order matters, the
//! case breaks two checks at once and asserts the **lower-numbered** reason, which is the one the
//! journal carries.
//!
//! Every test is pending until its story lands (DEC-77), and every one fails on the stubs because the
//! stub returns an error where the case expects a decision.

mod common;

use std::collections::BTreeSet;

use common::{
    AS_OF, DISCLOSURE_B, DISCLOSURE_C, EXPIRES_AT, HORIZON_S, INSTRUMENT_1, INSTRUMENT_2,
    INSTRUMENT_3, INSTRUMENT_4, INSTRUMENT_5, INSTRUMENT_9, Scenario, asset, at, dec, digest,
    equity_facts, proposal, revision, source, thesis_id, universe_of, usd,
};
use mandate_research::{
    AdmissionChange, AssetClass, AutonomyDecision, Corroboration, Direction, GroupId, Lineage,
    LineageState, PolicyOverlay, RefusalReason, ResearchEvent, UniverseChange,
    UniverseChangeReason, WorkingUniverse, admit, checks, group_of,
};

/// Asserts the decision refused for exactly this reason, and that `checks` agrees the reason is the
/// lowest-numbered failure. One helper, so no case can assert the reason without asserting the order.
fn assert_refused(s: &Scenario, expected: RefusalReason) {
    let a = admit(&s.input()).expect("the crate decides");
    assert_eq!(
        a.decision.reason(),
        Some(expected),
        "§8.5 check {} decides",
        expected.check_number()
    );
    assert!(
        a.first_order.is_none(),
        "a refusal proposes no order, so there are no first-order facts"
    );
    let all = checks(&s.input()).expect("the checks are total");
    let first = all
        .iter()
        .find(|c| c.failed)
        .map(|c| c.reason)
        .expect("a refusal has a failing check");
    assert_eq!(
        first, expected,
        "the journaled reason is the lowest-numbered failing check, not any failing check"
    );
}

#[test]
fn mc_n01_a_corroborated_thesis_in_an_allowed_asset_class_is_admitted() {
    let s = Scenario::admitting();

    let a = admit(&s.input()).expect("the crate decides");
    assert_eq!(
        a.decision,
        mandate_research::AdmissionDecision::Admitted {
            change: AdmissionChange::Admitted
        },
        "MC-N01 admits instrument 5 into an empty universe"
    );
    assert_eq!(
        a.universe,
        universe_of(&[INSTRUMENT_5]),
        "the universe gains exactly the admitted instrument"
    );
    let facts = a
        .first_order
        .expect("an admission carries the first order's facts");
    assert_eq!(
        (facts.new_instrument, facts.admission_ceiling),
        (true, AutonomyDecision::Ask),
        "MC-N01's first_order_autonomy is ask by the admission ceiling, which is this envelope's `ask`"
    );
    assert_eq!(
        facts.thesis_confidence,
        dec("0.8"),
        "the ceiling's companion fact is the thesis's own self-reported confidence"
    );
}

#[test]
fn mc_n01_admission_journals_the_thesis_entry_and_the_universe_change() {
    let s = Scenario::admitting();

    let a = admit(&s.input()).expect("the crate decides");
    let ResearchEvent::ThesisProposed(entry) = &a.journal[0] else {
        panic!("a revision-0 thesis journals ThesisProposed");
    };
    assert_eq!(
        (entry.admitted, entry.reason, entry.revision),
        (true, None, 0),
        "the entry records the verdict the checks reached"
    );
    assert_eq!(
        entry.asset_class,
        AssetClass::UsEquity,
        "the entry's asset class comes from instrument reference data (DEC-132 item 6)"
    );
    assert_eq!(
        entry.corroboration,
        Some(Corroboration::IndependentSource),
        "DEC-101: the corroboration the platform found is recorded in the entry"
    );
    let ResearchEvent::UniverseChanged(change) = &a.journal[1] else {
        panic!("an admission journals UniverseChanged into the account stream");
    };
    assert_eq!(
        (change.change, change.reason, change.universe_size_after),
        (
            UniverseChange::Admitted,
            UniverseChangeReason::ThesisAdmitted,
            1
        ),
        "the size is counted after the change, not before"
    );
}

#[test]
fn check_1_a_short_thesis_is_ignored() {
    let mut s = Scenario::admitting();
    s.proposal.thesis.direction = Direction::Other;

    assert_refused(&s, RefusalReason::DirectionNotAllowed);
    let a = admit(&s.input()).expect("the crate decides");
    assert!(
        a.decision.ignored(),
        "§8.2: a direction other than long is an ignored output as well as a refusal"
    );
}

#[test]
fn check_2_an_expiry_that_disagrees_with_the_horizon_is_ignored() {
    let mut s = Scenario::admitting();
    s.proposal.thesis.output.expires_at = at("2026-09-23T14:00:01.000000000Z");

    assert_refused(&s, RefusalReason::HorizonMismatch);
    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .ignored(),
        "§8.2 pairs expires_at with as_of + horizon_s, so one second apart is ignored"
    );
}

#[test]
fn check_3_a_revision_without_a_predecessor_is_ignored() {
    let mut s = Scenario::admitting();
    s.proposal.thesis.revision = 1;
    s.proposal.thesis.predecessor_thesis_id = None;

    assert_refused(&s, RefusalReason::RevisionWithoutPredecessor);
}

/// The other half of check 3, which a test for the missing predecessor alone would not reach.
#[test]
fn check_3_a_revision_zero_carrying_a_predecessor_is_ignored() {
    let mut s = Scenario::admitting();
    s.proposal.thesis.predecessor_thesis_id = Some(thesis_id("th-0"));

    assert_refused(&s, RefusalReason::RevisionWithoutPredecessor);
}

#[test]
fn check_4_no_admitting_model_refuses() {
    let s = Scenario::admitting().with_envelope(|e| {
        e.admits_instruments = false;
        e.research = None;
    });

    assert_refused(&s, RefusalReason::ResearchDisabled);
}

/// The overlay's `research_agent_allowed` reaches check 4 as well, which only ever refuses more
/// (DEC-132 item 22). No committed case exercises it.
#[test]
fn check_4_a_policy_that_forbids_the_research_agent_refuses() {
    let mut s = Scenario::admitting();
    s.overlay = PolicyOverlay {
        research_agent_allowed: false,
        admission_auto_allowed: true,
        ..PolicyOverlay::default()
    };

    assert_refused(&s, RefusalReason::ResearchDisabled);
}

/// MC-N08's base pins the universe *and* clears the research envelope, because V-036 with V-037
/// require it, so check 4 decides. Check 5 is only reachable for an input nobody validated.
#[test]
fn mc_n08_a_pinned_validated_mandate_refuses_at_check_four() {
    let s = Scenario::admitting().with_envelope(|e| {
        e.universe_pinned = true;
        e.max_instruments = 1;
        e.admits_instruments = false;
        e.research = None;
    });

    assert_refused(&s, RefusalReason::ResearchDisabled);
}

#[test]
fn check_5_a_pinned_universe_admits_nothing_even_with_an_admitting_model() {
    let s = Scenario::admitting().with_envelope(|e| e.universe_pinned = true);

    assert_refused(&s, RefusalReason::UniversePinned);
}

#[test]
fn check_6_admission_deny_refuses_outright() {
    let s = Scenario::admitting().with_envelope(|e| e.admission = AutonomyDecision::Deny);

    assert_refused(&s, RefusalReason::AdmissionDenied);
}

/// The overlay tightens `auto` to `ask` and never to `deny`, so a profile with
/// `admission_auto_allowed: false` refuses nothing at check 6 but raises the ceiling (MI-17).
#[test]
fn the_internal_research_profile_makes_every_admission_ask_without_refusing_any() {
    let mut s = Scenario::admitting().with_envelope(|e| e.admission = AutonomyDecision::Auto);
    s.overlay = PolicyOverlay::internal_research_profile();

    let a = admit(&s.input()).expect("the crate decides");
    assert!(
        a.decision.admitted(),
        "DEC-103's profile admits; it does not deny"
    );
    assert_eq!(
        a.first_order
            .expect("an admission carries the first order's facts")
            .admission_ceiling,
        AutonomyDecision::Ask,
        "DEC-103: every admission in the thin slice is ask, through admission_auto_allowed"
    );
}

#[test]
fn check_7_the_cost_cap_binds_at_equality() {
    let mut s = Scenario::admitting();
    s.facts.research_spend_usd_today = usd("5");

    assert_refused(&s, RefusalReason::CostCapReached);
}

#[test]
fn check_7_a_cent_below_the_cap_still_admits() {
    let mut s = Scenario::admitting();
    s.facts.research_spend_usd_today = usd("4.99");

    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .admitted(),
        "DEC-120 refuses at the cap, not before it"
    );
}

#[test]
fn check_8_the_thin_slices_data_universe_refuses_anything_outside_it() {
    let mut s = Scenario::admitting();
    s.facts.data_universe = Some(BTreeSet::from([asset(INSTRUMENT_2), asset(INSTRUMENT_3)]));

    assert_refused(&s, RefusalReason::NotInDataUniverse);
}

#[test]
fn check_9_an_operator_halt_refuses_the_admission() {
    let mut s = Scenario::admitting();
    s.facts.halted_instruments = BTreeSet::from([asset(INSTRUMENT_5)]);

    assert_refused(&s, RefusalReason::OperatorHalt);
}

#[test]
fn check_10_an_asset_class_outside_the_envelope_is_refused() {
    let mut s = Scenario::admitting();
    s.proposal.instrument.asset_class = AssetClass::Crypto;

    assert_refused(&s, RefusalReason::NotAllowedAssetClass);
}

/// The thesis cannot talk its way past check 10: the class the check reads is reference data, and a
/// thesis has no field to disagree with it (DEC-132 item 6). This test asserts the shape rather than
/// a value, because the hole is unrepresentable.
#[test]
fn check_10_reads_reference_data_and_a_thesis_has_no_asset_class_to_claim() {
    let mut s = Scenario::admitting();
    s.proposal.instrument.asset_class = AssetClass::Crypto;
    let claimed = s.proposal.thesis.clone();

    assert_refused(&s, RefusalReason::NotAllowedAssetClass);
    let a = admit(&s.input()).expect("the crate decides");
    let ResearchEvent::ThesisProposed(entry) = &a.journal[0] else {
        panic!("a revision-0 thesis journals ThesisProposed");
    };
    assert_eq!(
        entry.asset_class,
        AssetClass::Crypto,
        "the journaled class is reference data's, for the same thesis {claimed:?} whichever class it might have preferred"
    );
}

#[test]
fn mc_n16_a_leveraged_etp_without_the_owner_opt_in_is_refused() {
    let mut s = Scenario::admitting();
    s.proposal.instrument.leveraged_or_inverse_etp = true;

    assert_refused(&s, RefusalReason::LeveragedEtpNotEnabled);
}

#[test]
fn mc_n25_a_leveraged_etp_with_a_different_accepted_disclosure_version_is_refused() {
    let mut s = Scenario::admitting().with_envelope(|e| {
        e.leveraged_etps_enabled = true;
        e.leveraged_etp_disclosure_version = Some(digest(DISCLOSURE_B));
    });
    s.proposal.instrument.leveraged_or_inverse_etp = true;
    s.facts.disclosures_accepted = BTreeSet::from([digest(DISCLOSURE_C)]);

    assert_refused(&s, RefusalReason::LeveragedEtpNotEnabled);
}

#[test]
fn mc_n26_a_leveraged_etp_with_the_opt_in_and_the_exact_disclosure_is_admitted() {
    let mut s = Scenario::admitting().with_envelope(|e| {
        e.leveraged_etps_enabled = true;
        e.leveraged_etp_disclosure_version = Some(digest(DISCLOSURE_B));
    });
    s.proposal.instrument.leveraged_or_inverse_etp = true;
    s.facts.disclosures_accepted = BTreeSet::from([digest(DISCLOSURE_B)]);

    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .admitted(),
        "V-005's condition is the opt-in and a DisclosureAccepted for exactly that version"
    );
}

/// The opt-in without any accepted disclosure is still a refusal: both halves of V-005's condition
/// are required, which a test for the wrong version alone would not show.
#[test]
fn check_11_the_opt_in_alone_does_not_admit_a_leveraged_etp() {
    let mut s = Scenario::admitting().with_envelope(|e| {
        e.leveraged_etps_enabled = true;
        e.leveraged_etp_disclosure_version = Some(digest(DISCLOSURE_B));
    });
    s.proposal.instrument.leveraged_or_inverse_etp = true;

    assert_refused(&s, RefusalReason::LeveragedEtpNotEnabled);
}

#[test]
fn check_12_a_thesis_failing_the_eligibility_floor_is_refused() {
    let mut s = Scenario::admitting();
    s.facts.eligibility_failures = BTreeSet::from([asset(INSTRUMENT_5)]);

    assert_refused(&s, RefusalReason::EligibilityFloor);
}

#[test]
fn check_13_an_instrument_group_claimed_by_another_agent_is_refused() {
    let mut s = Scenario::admitting();
    s.facts.claimed_by_other_agents = BTreeSet::from([asset(INSTRUMENT_5)]);

    assert_refused(&s, RefusalReason::InstrumentGroupClaimed);
}

/// A claim on a *sibling* in the same named group refuses too: the check compares groups, not
/// instruments (trading spec §7.1).
#[test]
fn check_13_a_group_claimed_through_a_sibling_refuses() {
    let mut s = Scenario::admitting();
    let group = GroupId::new("grp.megacap").expect("a fixture group id is not empty");
    s.facts
        .instrument_groups
        .insert(asset(INSTRUMENT_5), group.clone());
    s.facts.instrument_groups.insert(asset(INSTRUMENT_2), group);
    s.facts.claimed_by_other_agents = BTreeSet::from([asset(INSTRUMENT_2)]);

    assert_refused(&s, RefusalReason::InstrumentGroupClaimed);
}

/// An ungrouped instrument stands as its own group, and a named group whose id spells another
/// instrument's asset id claims nothing (DEC-132 item 12).
#[test]
fn check_13_an_ungrouped_instrument_is_its_own_group_and_a_group_id_spelling_an_asset_id_claims_nothing()
 {
    let mut s = Scenario::admitting();
    let spelled_like_an_asset =
        GroupId::new(INSTRUMENT_5).expect("a fixture group id is not empty");
    s.facts
        .instrument_groups
        .insert(asset(INSTRUMENT_2), spelled_like_an_asset);
    s.facts.claimed_by_other_agents = BTreeSet::from([asset(INSTRUMENT_2)]);

    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .admitted(),
        "instrument 5 is Ungrouped, and a GroupId that spells its id is a different value"
    );
    assert_ne!(
        group_of(&asset(INSTRUMENT_5), &s.facts.instrument_groups),
        group_of(&asset(INSTRUMENT_2), &s.facts.instrument_groups),
        "the two instruments are in different groups whatever the group id spells"
    );
}

#[test]
fn check_14_one_source_off_the_allowlist_refuses_the_whole_thesis() {
    let mut s = Scenario::admitting();
    s.proposal
        .thesis
        .evidence_sources
        .push(source("src.anonblog"));

    assert_refused(&s, RefusalReason::SourceNotAllowlisted);
}

/// DEC-101's point: a thesis citing only allowlisted sources still needs corroboration, and one
/// without it is refused whatever it cited.
#[test]
fn check_15_a_thesis_without_corroboration_is_refused() {
    let mut s = Scenario::admitting();
    s.proposal.corroboration = None;

    assert_refused(&s, RefusalReason::NoCorroboration);
}

#[test]
fn check_15_market_data_corroboration_admits_a_thesis_citing_no_sources() {
    let mut s = Scenario::admitting();
    s.proposal.thesis.evidence_sources.clear();
    s.proposal.corroboration = Some(Corroboration::MarketData);

    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .admitted(),
        "market-data corroboration cites no source, so check 14 is vacuous and check 15 carries it"
    );
}

#[test]
fn check_16_a_retired_lineage_admits_nothing_further() {
    let mut s = Scenario::admitting();
    s.proposal.thesis = revision("th-2", "th-1", 1, "th-1", INSTRUMENT_5);
    s.lineages = LineageState::from_parts(
        [(
            common::lineage_id("th-1"),
            Lineage {
                revisions: 3,
                admitted: 4,
                retired: true,
            },
        )]
        .into_iter()
        .collect(),
        [(common::lineage_id("th-1"), asset(INSTRUMENT_5))]
            .into_iter()
            .collect(),
    );

    assert_refused(&s, RefusalReason::LineageRetired);
}

#[test]
fn check_16_a_revision_past_the_cap_is_refused() {
    let mut s = Scenario::admitting();
    s.proposal.thesis = revision("th-5", "th-1", 4, "th-4", INSTRUMENT_5);

    assert_refused(&s, RefusalReason::LineageRetired);
}

#[test]
fn check_16_a_revision_at_the_cap_is_admitted() {
    let mut s = Scenario::admitting();
    s.proposal.thesis = revision("th-4", "th-1", 3, "th-3", INSTRUMENT_5);

    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .admitted(),
        "DEC-111 caps at `max_revisions_per_lineage`, so revision 3 of a cap-3 lineage still admits"
    );
}

/// A policy that lowers `max_revisions_per_lineage` below the mandate's binds, because the key is a
/// maximum (§4.3).
#[test]
fn check_16_a_policy_ceiling_lowers_the_revision_cap() {
    let mut s = Scenario::admitting();
    s.proposal.thesis = revision("th-3", "th-1", 2, "th-2", INSTRUMENT_5);
    s.overlay = PolicyOverlay {
        max_revisions_per_lineage: Some(1),
        research_agent_allowed: true,
        admission_auto_allowed: true,
        ..PolicyOverlay::default()
    };

    assert_refused(&s, RefusalReason::LineageRetired);
}

#[test]
fn mc_n02_a_full_universe_refuses_rather_than_displacing() {
    let mut s = Scenario::admitting();
    s.universe = universe_of(&[
        INSTRUMENT_1,
        INSTRUMENT_2,
        INSTRUMENT_3,
        INSTRUMENT_4,
        INSTRUMENT_9,
    ]);

    assert_refused(&s, RefusalReason::UniverseFull);
    let a = admit(&s.input()).expect("the crate decides");
    assert_eq!(
        a.universe,
        universe_of(&[
            INSTRUMENT_1,
            INSTRUMENT_2,
            INSTRUMENT_3,
            INSTRUMENT_4,
            INSTRUMENT_9
        ]),
        "a full universe never displaces an active instrument, or an agent could churn its book"
    );
}

#[test]
fn mc_n14_renewing_an_active_instrument_adds_no_second_entry() {
    let mut s = Scenario::admitting();
    s.universe = universe_of(&[INSTRUMENT_5]);

    let a = admit(&s.input()).expect("the crate decides");
    assert_eq!(
        a.decision,
        mandate_research::AdmissionDecision::Admitted {
            change: AdmissionChange::Renewed
        },
        "a thesis for an already active instrument is a renewal"
    );
    assert_eq!(
        a.universe,
        universe_of(&[INSTRUMENT_5]),
        "a renewal leaves the universe exactly as it was"
    );
    assert_eq!(
        a.journal.len(),
        1,
        "a renewal journals the thesis entry alone, with no UniverseChanged"
    );
    assert!(
        a.first_order
            .expect("a renewal still carries the first order's facts")
            .new_instrument,
        "MC-N14 expects the admission ceiling on a renewal too, so new_instrument stays true"
    );
}

/// Check 17 is the only one a renewal skips: at the ceiling, a renewal still admits.
#[test]
fn a_renewal_skips_only_the_full_check() {
    let mut s = Scenario::admitting();
    s.universe = universe_of(&[
        INSTRUMENT_1,
        INSTRUMENT_2,
        INSTRUMENT_3,
        INSTRUMENT_4,
        INSTRUMENT_5,
    ]);

    let a = admit(&s.input()).expect("the crate decides");
    assert!(
        a.decision.admitted(),
        "a renewal at the ceiling admits: check 17 counts entries, and a renewal adds none"
    );
    let all = checks(&s.input()).expect("the checks are total");
    assert!(
        all.iter().all(|c| !c.failed),
        "no other check treats a renewal differently, which is what makes check 17 the only exception"
    );
}

/// A lowered `max_instruments` refuses further admissions and removes nothing (DEC-132 item 14).
#[test]
fn a_lowered_ceiling_refuses_and_never_removes() {
    let mut s = Scenario::admitting();
    s.universe = universe_of(&[INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3]);
    s.overlay = PolicyOverlay {
        max_instruments: Some(2),
        research_agent_allowed: true,
        admission_auto_allowed: true,
        ..PolicyOverlay::default()
    };

    assert_refused(&s, RefusalReason::UniverseFull);
    let a = admit(&s.input()).expect("the crate decides");
    assert_eq!(
        a.universe,
        universe_of(&[INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3]),
        "an over-ceiling universe drains by expiry (MI-19); nothing here removes to fit a ceiling"
    );
    assert!(
        a.journal
            .iter()
            .all(|e| !matches!(e, ResearchEvent::UniverseChanged(_))),
        "a refusal journals no universe change"
    );
}

/// §8.5's order, shown where two checks fail at once: the lower number is the journaled reason.
#[test]
fn the_lower_numbered_check_decides_when_several_fail() {
    let mut s = Scenario::admitting();
    s.proposal.instrument.asset_class = AssetClass::Crypto;
    s.facts.eligibility_failures = BTreeSet::from([asset(INSTRUMENT_5)]);
    s.proposal.corroboration = None;

    assert_refused(&s, RefusalReason::NotAllowedAssetClass);
    let all = checks(&s.input()).expect("the checks are total");
    let failed: Vec<u8> = all.iter().filter(|c| c.failed).map(|c| c.number).collect();
    assert_eq!(
        failed,
        vec![10, 12, 15],
        "`checks` reports every failure, in order, so the order itself is testable"
    );
}

#[test]
fn checks_reports_all_seventeen_in_the_spec_order() {
    let s = Scenario::admitting();

    let all = checks(&s.input()).expect("the checks are total");
    let numbers: Vec<u8> = all.iter().map(|c| c.number).collect();
    assert_eq!(
        numbers,
        (1..=17).collect::<Vec<u8>>(),
        "every §8.5 check is evaluated and reported in its own right"
    );
    assert!(
        all.iter().all(|c| c.reason.check_number() == c.number),
        "each check's reason carries the ordinal it was reported under"
    );
}

/// §2.3: an unread universe is an error, never an admission and never an empty set.
#[test]
fn an_unavailable_universe_is_an_error() {
    let mut s = Scenario::admitting();
    s.universe = WorkingUniverse::Unavailable;

    let e = admit(&s.input()).expect_err("an unread universe cannot be decided");
    assert_eq!(
        e.code(),
        "universe_unavailable",
        "the crate refuses to decide rather than guessing the universe is empty"
    );
}

/// The entry type follows the revision number, not the verdict: an ignored revision is still a
/// `ThesisRevised` (MC-N19).
#[test]
fn the_entry_type_follows_the_revision_number() {
    let mut s = Scenario::admitting();
    s.proposal = proposal(revision("th-40", "th-40", 1, "th-39", INSTRUMENT_5));
    s.proposal.instrument = equity_facts();

    let a = admit(&s.input()).expect("the crate decides");
    assert!(
        matches!(a.journal[0], ResearchEvent::ThesisRevised(_)),
        "revision 1 journals ThesisRevised even when a later check refuses it"
    );
    let ResearchEvent::ThesisRevised(entry) = &a.journal[0] else {
        panic!("revision 1 journals ThesisRevised");
    };
    assert_eq!(
        entry.predecessor_thesis_id.as_ref(),
        Some(&thesis_id("th-39")),
        "journal spec §9: a revision records its predecessor"
    );
    assert_eq!(
        entry.allowlist_version, s.facts.allowlist.version,
        "DEC-101: the allowlist version in effect is recorded in every entry"
    );
}

/// The thesis's own timestamps are what check 2 reads, and the horizon pairs them exactly (DEC-118).
#[test]
fn the_horizon_pairs_as_of_with_expires_at_exactly() {
    let mut s = Scenario::admitting();
    s.proposal.thesis.horizon_s = HORIZON_S;
    s.proposal.thesis.output.as_of = at(AS_OF);
    s.proposal.thesis.output.expires_at = at(EXPIRES_AT);

    assert!(
        admit(&s.input())
            .expect("the crate decides")
            .decision
            .admitted(),
        "86400 s after 2026-09-22T14:00:00Z is 2026-09-23T14:00:00Z, so check 2 passes"
    );
}
