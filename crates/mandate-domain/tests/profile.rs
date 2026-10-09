//! The capability profile (E7-23 B1, DEC-531, DEC-630): its refusals, its canonical object, and
//! a hash that follows the profile's content and nothing else.

use std::collections::BTreeSet;

use mandate_canon::{Digest, to_canonical};
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType, ProfileError,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce,
};
use proptest::prelude::*;

fn cell(
    order_type: OrderType,
    quantity_form: QuantityForm,
    tifs: &[TimeInForce],
    protection: &[ProtectionForm],
) -> Cell {
    Cell {
        order_type,
        quantity_form,
        times_in_force: tifs.iter().copied().collect(),
        protection_forms: protection.iter().copied().collect(),
    }
}

/// Three rows, given out of canonical order, and a broker whose retries are of unknown effect.
///
/// The two US equity rows pin the row order: sorted by asset class then session, `us_equity`'s
/// `after_hours` row comes after `crypto` and before `regular`. Sorted by session first, or by the
/// sessions' declaration order, it would not.
fn rows() -> Vec<Row> {
    vec![
        Row {
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            cells: vec![
                cell(
                    OrderType::Market,
                    QuantityForm::Notional,
                    &[TimeInForce::Day],
                    &[],
                ),
                cell(
                    OrderType::Limit,
                    QuantityForm::Whole,
                    &[TimeInForce::Gtc, TimeInForce::Day],
                    &[ProtectionForm::Oco, ProtectionForm::Bracket],
                ),
            ],
        },
        Row {
            asset_class: AssetClass::Crypto,
            session: MarketSession::Crypto,
            cells: vec![cell(
                OrderType::StopLimit,
                QuantityForm::Fractional,
                &[TimeInForce::Ioc, TimeInForce::Gtc],
                &[ProtectionForm::StopLimit],
            )],
        },
        Row {
            asset_class: AssetClass::UsEquity,
            session: MarketSession::AfterHours,
            cells: vec![cell(
                OrderType::Limit,
                QuantityForm::Whole,
                &[TimeInForce::Day, TimeInForce::Gtc],
                &[],
            )],
        },
    ]
}

const IDEMPOTENCY: Idempotency = Idempotency {
    client_order_id: true,
    retry: Retry::Unknown,
    query_by_client_order_id: false,
};

/// [`rows`] and [`IDEMPOTENCY`] at version 2, written by hand from DEC-630 item 6: keys sorted
/// by bytes, rows by asset class then session, cells by order type then quantity form, and sets
/// by their spelling.
const CANONICAL: &str = concat!(
    r#"{"idempotency":{"client_order_id":true,"query_by_client_order_id":false,"retry":"unknown"},"#,
    r#""kind":"broker_profile","profile_version":2,"rows":["#,
    r#"{"asset_class":"crypto","cells":[{"order_type":"stop_limit","protection_forms":["stop_limit"],"#,
    r#""quantity_form":"fractional","times_in_force":["gtc","ioc"]}],"session":"crypto"},"#,
    r#"{"asset_class":"us_equity","cells":[{"order_type":"limit","protection_forms":[],"#,
    r#""quantity_form":"whole","times_in_force":["day","gtc"]}],"session":"after_hours"},"#,
    r#"{"asset_class":"us_equity","cells":["#,
    r#"{"order_type":"limit","protection_forms":["bracket","oco"],"quantity_form":"whole","#,
    r#""times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"market","protection_forms":[],"quantity_form":"notional","#,
    r#""times_in_force":["day"]}],"session":"regular"}]}"#,
);

#[test]
fn the_canonical_object_is_dec_630s_and_the_hash_is_its_sha256() {
    let profile = CapabilityProfile::new(2, rows(), IDEMPOTENCY).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical(profile.canonical())).unwrap(),
        CANONICAL
    );
    assert_eq!(profile.content_hash(), Digest::of(CANONICAL.as_bytes()));
}

/// One change to a profile's version, rows or idempotency.
type Edit = fn(&mut u32, &mut Vec<Row>, &mut Idempotency);

#[test]
fn every_member_moves_the_hash() {
    let base = CapabilityProfile::new(2, rows(), IDEMPOTENCY)
        .unwrap()
        .content_hash();
    let edits: [(&str, Edit); 14] = [
        ("profile_version", |v, _, _| *v = 3),
        ("retry idempotent", |_, _, i| i.retry = Retry::Idempotent),
        ("retry not idempotent", |_, _, i| {
            i.retry = Retry::NotIdempotent
        }),
        ("query by client id", |_, _, i| {
            i.query_by_client_order_id = true
        }),
        ("no client id", |_, _, i| {
            *i = Idempotency {
                client_order_id: false,
                retry: Retry::NotIdempotent,
                ..*i
            }
        }),
        ("session", |_, r, _| r[0].session = MarketSession::PreMarket),
        ("asset class", |_, r, _| {
            r[1].asset_class = AssetClass::UsEquity
        }),
        ("order type", |_, r, _| {
            r[0].cells[1].order_type = OrderType::Stop
        }),
        ("quantity form", |_, r, _| {
            r[0].cells[1].quantity_form = QuantityForm::Fractional
        }),
        ("time in force", |_, r, _| {
            r[0].cells[0].times_in_force.insert(TimeInForce::Ioc);
        }),
        ("protection removed", |_, r, _| {
            r[0].cells[1].protection_forms.remove(&ProtectionForm::Oco);
        }),
        ("protection added", |_, r, _| {
            r[0].cells[0]
                .protection_forms
                .insert(ProtectionForm::StopLimit);
        }),
        ("cell added", |_, r, _| {
            r[1].cells.push(cell(
                OrderType::Limit,
                QuantityForm::Whole,
                &[TimeInForce::Gtc],
                &[],
            ));
        }),
        ("row added", |_, r, _| {
            r.push(Row {
                session: MarketSession::PreMarket,
                ..r[0].clone()
            });
        }),
    ];
    let mut seen = BTreeSet::from([base]);
    for (name, edit) in edits {
        let (mut version, mut rows, mut idempotency) = (2, rows(), IDEMPOTENCY);
        edit(&mut version, &mut rows, &mut idempotency);
        let hash = CapabilityProfile::new(version, rows, idempotency)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
            .content_hash();
        assert!(
            seen.insert(hash),
            "{name} left the hash where another profile has it"
        );
    }
}

#[test]
fn each_defect_is_refused_with_its_own_reason() {
    let only = |cells: Vec<Cell>| {
        vec![Row {
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            cells,
        }]
    };
    let whole = || {
        cell(
            OrderType::Limit,
            QuantityForm::Whole,
            &[TimeInForce::Day],
            &[],
        )
    };
    let refuse = |version, rows, idempotency, reason| {
        assert_eq!(
            CapabilityProfile::new(version, rows, idempotency),
            Err(reason)
        );
    };
    refuse(0, rows(), IDEMPOTENCY, ProfileError::VersionZero);
    refuse(1, vec![], IDEMPOTENCY, ProfileError::NoRows);
    refuse(
        1,
        [only(vec![whole()]), only(vec![whole()])].concat(),
        IDEMPOTENCY,
        ProfileError::DuplicateRow,
    );
    refuse(1, only(vec![]), IDEMPOTENCY, ProfileError::EmptyRow);
    refuse(
        1,
        only(vec![whole(), whole()]),
        IDEMPOTENCY,
        ProfileError::DuplicateCell,
    );
    let no_tif = cell(OrderType::Limit, QuantityForm::Whole, &[], &[]);
    refuse(
        1,
        only(vec![no_tif]),
        IDEMPOTENCY,
        ProfileError::NoTimeInForce,
    );
    let without = Idempotency {
        client_order_id: false,
        retry: Retry::NotIdempotent,
        query_by_client_order_id: false,
    };
    for claim in [
        Idempotency {
            retry: Retry::Idempotent,
            ..without
        },
        Idempotency {
            retry: Retry::Unknown,
            ..without
        },
        Idempotency {
            query_by_client_order_id: true,
            ..without
        },
    ] {
        refuse(
            1,
            only(vec![whole()]),
            claim,
            ProfileError::ClaimWithoutClientId,
        );
    }
    assert!(CapabilityProfile::new(1, only(vec![whole()]), without).is_ok());
}

#[test]
fn every_profile_error_has_a_distinct_stable_code() {
    let errors = [
        ProfileError::Unimplemented,
        ProfileError::VersionZero,
        ProfileError::NoRows,
        ProfileError::DuplicateRow,
        ProfileError::EmptyRow,
        ProfileError::DuplicateCell,
        ProfileError::NoTimeInForce,
        ProfileError::ClaimWithoutClientId,
        ProfileError::NotOffered,
        ProfileError::NonCanonical,
    ];
    let codes: BTreeSet<&str> = errors.iter().map(|e| e.code()).collect();
    assert_eq!(codes.len(), errors.len(), "ES-09: one code per variant");
    assert!(codes.iter().all(|c| !c.is_empty() && c.is_ascii()));
}

#[test]
fn only_an_idempotent_broker_may_be_sent_an_order_again_blindly() {
    assert!(Retry::Idempotent.may_resend_blindly());
    assert!(
        !Retry::NotIdempotent.may_resend_blindly(),
        "a re-send could be a second order"
    );
    assert!(
        !Retry::Unknown.may_resend_blindly(),
        "DEC-630 item 2: unknown is read as not idempotent"
    );
}

#[test]
fn a_profile_reads_back_its_idempotency_and_the_one_cell_for_an_order() {
    let profile = CapabilityProfile::new(2, rows(), IDEMPOTENCY).unwrap();
    assert_eq!(profile.idempotency(), IDEMPOTENCY);
    let read = |asset_class, session, order_type, quantity_form| {
        profile.cell(asset_class, session, order_type, quantity_form)
    };
    let regular_limit = cell(
        OrderType::Limit,
        QuantityForm::Whole,
        &[TimeInForce::Day, TimeInForce::Gtc],
        &[ProtectionForm::Bracket, ProtectionForm::Oco],
    );
    assert_eq!(
        read(
            AssetClass::UsEquity,
            MarketSession::Regular,
            OrderType::Limit,
            QuantityForm::Whole
        ),
        Ok(&regular_limit)
    );
    let after_hours_limit = cell(
        OrderType::Limit,
        QuantityForm::Whole,
        &[TimeInForce::Day, TimeInForce::Gtc],
        &[],
    );
    assert_eq!(
        read(
            AssetClass::UsEquity,
            MarketSession::AfterHours,
            OrderType::Limit,
            QuantityForm::Whole
        ),
        Ok(&after_hours_limit),
        "the same order in another session is another cell"
    );
    let crypto_stop_limit = cell(
        OrderType::StopLimit,
        QuantityForm::Fractional,
        &[TimeInForce::Gtc, TimeInForce::Ioc],
        &[ProtectionForm::StopLimit],
    );
    assert_eq!(
        read(
            AssetClass::Crypto,
            MarketSession::Crypto,
            OrderType::StopLimit,
            QuantityForm::Fractional
        ),
        Ok(&crypto_stop_limit)
    );
    for (asset_class, session, order_type, quantity_form) in [
        (
            AssetClass::UsEquity,
            MarketSession::PreMarket,
            OrderType::Limit,
            QuantityForm::Whole,
        ),
        (
            AssetClass::Crypto,
            MarketSession::Regular,
            OrderType::Limit,
            QuantityForm::Whole,
        ),
        (
            AssetClass::UsEquity,
            MarketSession::Regular,
            OrderType::Stop,
            QuantityForm::Whole,
        ),
        (
            AssetClass::UsEquity,
            MarketSession::Regular,
            OrderType::Limit,
            QuantityForm::Fractional,
        ),
    ] {
        assert_eq!(
            read(asset_class, session, order_type, quantity_form),
            Err(ProfileError::NotOffered),
            "{asset_class:?} {session:?} {order_type:?} {quantity_form:?} is not listed"
        );
    }
}

fn valid_profile() -> impl Strategy<Value = (u32, Vec<Row>, Idempotency)> {
    let tifs = proptest::sample::subsequence(
        vec![TimeInForce::Day, TimeInForce::Gtc, TimeInForce::Ioc],
        1..=3,
    );
    let forms = proptest::sample::subsequence(
        vec![
            ProtectionForm::Bracket,
            ProtectionForm::Oco,
            ProtectionForm::StopLimit,
        ],
        0..=3,
    );
    let keys: Vec<(OrderType, QuantityForm)> = [
        OrderType::Market,
        OrderType::Limit,
        OrderType::Stop,
        OrderType::StopLimit,
    ]
    .into_iter()
    .flat_map(|o| {
        [
            QuantityForm::Whole,
            QuantityForm::Fractional,
            QuantityForm::Notional,
        ]
        .map(|q| (o, q))
    })
    .collect();
    let cells = proptest::sample::subsequence(keys, 1..=3).prop_flat_map(move |keys| {
        proptest::collection::vec((tifs.clone(), forms.clone()), keys.len()).prop_map(move |sets| {
            keys.iter()
                .zip(sets)
                .map(|(&(o, q), (t, p))| cell(o, q, &t, &p))
                .collect::<Vec<_>>()
        })
    });
    let places = proptest::sample::subsequence(
        vec![
            (AssetClass::UsEquity, MarketSession::Regular),
            (AssetClass::UsEquity, MarketSession::PreMarket),
            (AssetClass::UsEquity, MarketSession::AfterHours),
            (AssetClass::Crypto, MarketSession::Crypto),
        ],
        1..=4,
    );
    let rows = places.prop_flat_map(move |places| {
        proptest::collection::vec(cells.clone(), places.len()).prop_map(move |cells| {
            places
                .iter()
                .zip(cells)
                .map(|(&(asset_class, session), cells)| Row {
                    asset_class,
                    session,
                    cells,
                })
                .collect()
        })
    });
    let idempotency =
        (any::<bool>(), 0..3usize, any::<bool>()).prop_map(|(id, retry, query)| Idempotency {
            client_order_id: id,
            retry: if id {
                [Retry::Idempotent, Retry::NotIdempotent, Retry::Unknown][retry]
            } else {
                Retry::NotIdempotent
            },
            query_by_client_order_id: id && query,
        });
    (1..4u32, rows, idempotency)
}

proptest! {
    #[test]
    fn the_order_rows_and_cells_are_given_in_is_not_part_of_the_profile(
        (version, rows, idempotency) in valid_profile(),
        seed in any::<u64>(),
    ) {
        let mut shuffled = rows.clone();
        let turn = usize::try_from(seed).unwrap() % shuffled.len();
        shuffled.rotate_left(turn);
        for row in &mut shuffled {
            row.cells.reverse();
        }
        let a = CapabilityProfile::new(version, rows, idempotency).unwrap();
        let b = CapabilityProfile::new(version, shuffled, idempotency).unwrap();
        prop_assert_eq!(a.content_hash(), b.content_hash());
        prop_assert_eq!(a, b);
    }
}
