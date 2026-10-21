//! `mandate step-up confirm`'s flags (DEC-2104 item 2, DEC-2106 items 2.1 to 2.4; E9-4 S2): what
//! [`check`] refuses before any journal is opened, and how [`normalize`] reads the code the web app
//! shows. The expected codes are computed here: the property test encodes 40 bits as Crockford base
//! 32 itself. What the command does with the journal is `tests/step_up_confirm.rs`.

use mandate_cli::control::ControlError;
use mandate_cli::step_up::{Confirming, check, normalize};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// DEC-2106 items 2.1 to 2.4, first failing check first, before any journal exists.
#[test]
fn the_flags_are_checked_in_order_before_any_journal() {
    let base = Confirming {
        workspace: "ws1",
        principal: "01J9ZP000000000000000000PR",
        agent: "agent-a",
        approval: "01000000000000000000000001",
        challenge: "01J9ZC000000000000000000CH",
        code: "ABCD-EFGH",
    };
    assert!(check(&base).is_ok(), "{:?}", check(&base));
    let cases = [
        ("workspace=ws 1", "owner_workspace_invalid"),
        ("principal=01j9zp000000000000000000pr", "principal_invalid"),
        ("challenge=81J9ZC000000000000000000CH", "challenge_invalid"),
        ("code=ABCD-EFGU", "code_invalid"),
        ("workspace=,principal=", "owner_workspace_invalid"),
        ("principal=x,challenge=y", "principal_invalid"),
        ("challenge=y,code=", "challenge_invalid"),
    ];
    for (changes, reason) in cases {
        let mut case = base;
        for (flag, value) in changes.split(',').filter_map(|c| c.split_once('=')) {
            match flag {
                "workspace" => case.workspace = value,
                "principal" => case.principal = value,
                "challenge" => case.challenge = value,
                _ => case.code = value,
            }
        }
        assert_eq!(check(&case).err(), Some(refused(reason)), "{changes}");
    }
}

/// DEC-2106 item 2.4: the one displayed form or the bare 8, any case, `I` and `L` as `1`, `O` as
/// `0`; nothing else.
#[test]
fn a_code_normalizes_as_the_web_app_shows_it() {
    for (typed, normal) in [
        ("ABCD-EFGH", "ABCDEFGH"),
        ("abcdefgh", "ABCDEFGH"),
        ("o1lI-2345", "01112345"),
        ("zzzz-0000", "ZZZZ0000"),
    ] {
        assert_eq!(normalize(typed), Ok(normal.to_owned()), "{typed}");
    }
    let refused_forms = "|ABCD-EFG|ABCDEFGHJ|ABC-DEFGH|ABCDE-FGH|ABCD--EFGH|ABCD EFGH|ABCDEFGU|\
                         -ABCDEFG|ABCDEFG-|ABCD_EFGH|ABCD\u{c9}FGH";
    for typed in refused_forms.split('|') {
        assert_eq!(normalize(typed), Err(refused("code_invalid")), "{typed:?}");
    }
}

/// Any 40-bit code the web app can show normalizes to itself, displayed or bare, in either case,
/// with any of its `0`s and `1`s typed as the letters Crockford reads as them.
#[test]
fn any_shown_code_normalizes_to_itself_and_its_typed_forms_too() {
    let drawn = (0..1u64 << 40, any::<bool>(), any::<u8>(), any::<u8>());
    TestRunner::deterministic()
        .run(&drawn, |(bits, hyphen, cases, letters)| {
            let shown: String = (0..8)
                .map(|i| char::from(CROCKFORD[((bits >> (35 - 5 * i)) & 31) as usize]))
                .collect();
            let mut typed = String::new();
            for (i, c) in shown.chars().enumerate() {
                if hyphen && i == 4 {
                    typed.push('-');
                }
                let c = match (c, letters >> i & 1 == 1) {
                    ('0', true) => 'O',
                    ('1', true) => ['I', 'L'][i % 2],
                    (c, _) => c,
                };
                let lower = cases >> i & 1 == 1;
                typed.push(if lower { c.to_ascii_lowercase() } else { c });
            }
            prop_assert_eq!(normalize(&typed), Ok(shown), "{}", typed);
            Ok(())
        })
        .unwrap();
}
