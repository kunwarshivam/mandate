//! Recorded responses carry no credentials (AGENTS.md rule 7): no Alpaca key ID shape, no header
//! names, and, when the paper credentials are in the environment, neither value.

mod common;

use std::fs;

use mandate_marketdata::http::{KEY_ID_VAR, SECRET_VAR};

fn looks_like_key_id(word: &str) -> bool {
    word.len() == 20
        && (word.starts_with("PK") || word.starts_with("AK"))
        && word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[test]
fn recorded_fixtures_contain_no_credentials() {
    let secrets: Vec<String> = [KEY_ID_VAR, SECRET_VAR]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|value| !value.is_empty())
        .collect();
    let mut files = 0;
    for scenario in fs::read_dir(common::fixtures_dir()).unwrap() {
        let dir = scenario.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        for file in fs::read_dir(&dir).unwrap() {
            let path = file.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let lower = text.to_ascii_lowercase();
            assert!(
                !lower.contains("apca-api") && !lower.contains("secret"),
                "{}",
                path.display()
            );
            assert!(
                !text
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(looks_like_key_id),
                "{} holds something shaped like a key ID",
                path.display()
            );
            assert!(
                secrets.iter().all(|s| !text.contains(s.as_str())),
                "{} holds a credential value",
                path.display()
            );
            files += 1;
        }
    }
    assert!(files >= 14, "the scan must see every recorded page");
}
