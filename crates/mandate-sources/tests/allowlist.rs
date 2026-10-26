#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The allowlist file and the URL guard (E17-7, DP-9, DEC-433 items 11 and 20, DEC-1130 items 1 to
//! 3).

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use mandate_research::AllowlistVersion;
use mandate_sources::SourceError;
use mandate_sources::allowlist::{Allowlist, Approval, Approver, Endpoint, SourceKind};
use proptest::prelude::*;

use common::{TEST_ALLOWLIST, allowlist, edgar_source, news_source, source};

fn checked_in(version: u32) -> Allowlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../config/sources/allowlist-v{version}.toml"));
    Allowlist::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_checked_in_file_parses_and_its_version_is_its_file_name() {
    let list = checked_in(1);
    assert_eq!(list.version(), AllowlistVersion(1));
    let ids: Vec<&str> = list.entries().map(|e| e.id().as_str()).collect();
    assert_eq!(ids, ["alpaca.news", "sec.edgar"]);
}

#[test]
fn the_checked_in_file_keeps_alpaca_news_out_until_counsel_confirms() {
    let list = checked_in(1);
    assert!(list.active(&news_source()).is_none());
    assert_eq!(
        list.permit(
            &news_source(),
            "https://data.alpaca.markets/v1beta1/news?symbols=AAPL"
        ),
        Err(SourceError::SourceNotAllowlisted)
    );
    let research = list.research_allowlist();
    assert_eq!(research.version, AllowlistVersion(1));
    assert_eq!(research.sources, BTreeSet::from([edgar_source()]));
    let edgar = list.active(&edgar_source()).unwrap();
    assert_eq!(edgar.kind(), SourceKind::Filings);
    assert!(!edgar.licensed());
    assert_eq!(edgar.min_interval_ms(), 125);
}

/// DEC-2394: version 2 put Alpaca news in effect, the founder's answer to DEC-1130 item 2. It is
/// kept as merged, never edited, though its `counsel_confirmed` claim was wrong (DEC-2276): the same
/// source ids as version 1, each with the same kind, licence, interval, cap, and endpoints, and
/// Alpaca news now in effect and still licensed, so readers keep withholding its text (DEC-1130
/// item 6).
#[test]
fn version_two_admits_alpaca_news_and_keeps_its_text_licensed() {
    let (one, two) = (checked_in(1), checked_in(2));
    assert_eq!(two.version(), AllowlistVersion(2));
    let ids = |list: &Allowlist| -> Vec<String> {
        list.entries().map(|e| e.id().as_str().to_owned()).collect()
    };
    assert_eq!(ids(&one), ids(&two));
    for (before, after) in one.entries().zip(two.entries()) {
        assert_eq!(before.kind(), after.kind(), "{}", after.id().as_str());
        assert_eq!(
            before.licensed(),
            after.licensed(),
            "{}",
            after.id().as_str()
        );
        assert_eq!(before.min_interval_ms(), after.min_interval_ms());
        assert_eq!(before.max_response_bytes(), after.max_response_bytes());
        assert_eq!(before.endpoints(), after.endpoints());
    }
    let news = two.active(&news_source()).unwrap();
    assert_eq!(news.kind(), SourceKind::News);
    assert!(news.licensed(), "its text stays withheld");
    assert!(
        two.permit(
            &news_source(),
            "https://data.alpaca.markets/v1beta1/news?symbols=AAPL"
        )
        .is_ok()
    );
    assert_eq!(
        two.permit(&news_source(), "https://data.alpaca.markets/v2/stocks"),
        Err(SourceError::EndpointNotAllowlisted),
        "only the news endpoint"
    );
    let research = two.research_allowlist();
    assert_eq!(research.version, AllowlistVersion(2));
    assert_eq!(
        research.sources,
        BTreeSet::from([edgar_source(), news_source()])
    );
}

#[test]
fn the_research_allowlist_holds_exactly_the_confirmed_sources() {
    let research = allowlist().research_allowlist();
    assert_eq!(research.version, AllowlistVersion(7));
    assert_eq!(
        research.sources,
        BTreeSet::from([edgar_source(), news_source()])
    );
}

#[test]
fn permit_accepts_urls_under_an_endpoint_of_the_named_source() {
    let list = allowlist();
    for (id, url) in [
        (
            "sec.edgar",
            "https://data.sec.gov/submissions/CIK0000320193.json",
        ),
        (
            "sec.edgar",
            "https://www.sec.gov/Archives/edgar/data/320193/000032019326000090/aapl-8k-20261001.htm",
        ),
        (
            "alpaca.news",
            "https://data.alpaca.markets/v1beta1/news?symbols=AAPL,MSFT&start=2026-10-15T00:00:00Z&limit=50&sort=desc",
        ),
    ] {
        let permitted = list.permit(&source(id), url).unwrap();
        assert_eq!(permitted.as_str(), url);
        assert_eq!(permitted.source(), &source(id));
    }
    assert_eq!(
        list.permit(
            &news_source(),
            "https://data.alpaca.markets/v1beta1/news?symbols=AAPL"
        )
        .unwrap()
        .max_response_bytes(),
        1_048_576
    );
}

#[test]
fn permit_refuses_every_url_outside_the_endpoints() {
    let list = allowlist();
    let edgar = edgar_source();
    for url in [
        "http://data.sec.gov/submissions/CIK0000320193.json",
        "https://data.sec.gov.evil.example/submissions/x.json",
        "https://evil.example/https://data.sec.gov/submissions/",
        "https://user@data.sec.gov/submissions/x.json",
        "https://data.sec.gov@evil.example/submissions/x.json",
        "https://data.sec.gov:8443/submissions/x.json",
        "https://DATA.SEC.GOV/submissions/x.json",
        "https://data.sec.gov/submissions/../cgi-bin/x",
        "https://data.sec.gov/submissions/%2e%2e/cgi-bin/x",
        "https://data.sec.gov/submissionsX/x.json",
        "https://data.sec.gov/submissions",
        "https://data.sec.gov/other/x.json",
        "https://data.sec.gov/submissions/x.json#frag",
        "https://data.sec.gov/submissions/x\\..\\y",
        "https://data.sec.gov/submissions/x y.json",
        "https://data.sec.gov/submissions/x\n.json",
        "https://data.sec.gov//submissions/x.json",
        "https://data.alpaca.markets/v1beta1/news?symbols=AAPL",
        "",
    ] {
        assert_eq!(
            list.permit(&edgar, url),
            Err(SourceError::EndpointNotAllowlisted),
            "{url:?}"
        );
    }
}

#[test]
fn permit_refuses_unlisted_and_unconfirmed_sources_whatever_the_url() {
    let list = allowlist();
    assert_eq!(
        list.permit(
            &source("evil.blog"),
            "https://data.sec.gov/submissions/x.json"
        ),
        Err(SourceError::SourceNotAllowlisted)
    );
    assert_eq!(
        list.permit(
            &source("wire.unconfirmed"),
            "https://news.example.com/v1/latest"
        ),
        Err(SourceError::SourceNotAllowlisted)
    );
    assert!(list.active(&source("wire.unconfirmed")).is_none());
    assert_eq!(list.entries().count(), 3);
}

#[test]
fn malformed_files_are_refused() {
    let base = TEST_ALLOWLIST;
    let cases: Vec<(String, SourceError)> = vec![
        ("not toml".to_owned(), SourceError::AllowlistMalformed),
        (
            base.replace("version = 7", "version = 0"),
            SourceError::AllowlistMalformed,
        ),
        (
            base.replace("licensed = false", "licensed = false\nextra = 1"),
            SourceError::AllowlistMalformed,
        ),
        (
            base.replace("\"wire.unconfirmed\"", "\"sec.edgar\""),
            SourceError::DuplicateSource,
        ),
        (
            base.replace("\"wire.unconfirmed\"", "\"\""),
            SourceError::AllowlistMalformed,
        ),
        (
            base.replace(
                "https://news.example.com/v1/",
                "http://news.example.com/v1/",
            ),
            SourceError::BadEndpoint,
        ),
        (
            base.replace("[\"https://news.example.com/v1/\"]", "[]"),
            SourceError::BadEndpoint,
        ),
        (
            base.replace(
                "max_response_bytes = 1048576\nendpoints = [\"https://news",
                "max_response_bytes = 0\nendpoints = [\"https://news",
            ),
            SourceError::AllowlistMalformed,
        ),
        (
            base.replace("kind = \"filings\"", "kind = \"social\""),
            SourceError::AllowlistMalformed,
        ),
    ];
    for (text, expected) in cases {
        assert_ne!(text, base);
        assert_eq!(Allowlist::parse(&text), Err(expected), "{text}");
    }
}

#[test]
fn endpoints_parse_only_as_an_https_host_and_a_path_prefix() {
    let endpoint = Endpoint::parse("https://data.sec.gov/submissions/").unwrap();
    assert_eq!(endpoint.host(), "data.sec.gov");
    assert_eq!(endpoint.path_prefix(), "/submissions/");
    let bare = Endpoint::parse("https://data.alpaca.markets/v1beta1/news").unwrap();
    assert_eq!(bare.path_prefix(), "/v1beta1/news");
    for bad in [
        "http://data.sec.gov/x/",
        "https://data.sec.gov",
        "https:///x/",
        "https://Data.sec.gov/x/",
        "https://data.sec.gov:443/x/",
        "https://u@data.sec.gov/x/",
        "https://data.sec.gov/x/../",
        "https://data.sec.gov/x?y",
        "https://data_sec.gov/x/",
        "ftp://data.sec.gov/x/",
    ] {
        assert_eq!(Endpoint::parse(bad), Err(SourceError::BadEndpoint), "{bad}");
    }
}

/// The allowed endpoints as literal strings: the oracle knows nothing of the guard's parser.
const ROOTS: [&str; 2] = [
    "https://data.sec.gov/submissions/",
    "https://www.sec.gov/Archives/edgar/data/",
];

fn oracle_allows(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    ROOTS.iter().any(|root| url.starts_with(root))
        && !rest.contains("..")
        && !rest.contains("//")
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~/?&=:,+".contains(c))
}

proptest! {
    #[test]
    fn a_permitted_url_always_lies_under_an_endpoint(
        root in prop::sample::select(vec![
            "https://data.sec.gov/submissions/",
            "https://www.sec.gov/Archives/edgar/data/",
            "https://data.sec.gov/",
            "https://data.sec.gov.evil.example/submissions/",
            "https://www.sec.gov/",
            "http://data.sec.gov/submissions/",
        ]),
        tail in "[a-zA-Z0-9./%?&=:@#\\\\ -]{0,24}",
    ) {
        let url = format!("{root}{tail}");
        let permitted = allowlist().permit(&edgar_source(), &url).is_ok();
        prop_assert_eq!(permitted, oracle_allows(&url), "{}", url);
    }
}

/// DEC-2276: version 3 records the founder's approval of Alpaca news as his, with the decision
/// that records it, and counsel's confirmation as not given; it is version 2's sources otherwise,
/// field for field, and Alpaca news is in effect exactly as in version 2, its text still licensed.
#[test]
fn version_three_records_the_founder_s_approval_and_no_counsel_confirmation() {
    let (two, three) = (checked_in(2), checked_in(3));
    assert_eq!(three.version(), AllowlistVersion(3));
    for (before, after) in two.entries().zip(three.entries()) {
        assert_eq!(before.id(), after.id());
        assert_eq!(before.kind(), after.kind(), "{}", after.id().as_str());
        assert_eq!(before.licensed(), after.licensed());
        assert_eq!(before.min_interval_ms(), after.min_interval_ms());
        assert_eq!(before.max_response_bytes(), after.max_response_bytes());
        assert_eq!(before.endpoints(), after.endpoints());
        assert_eq!(before.in_effect(), after.in_effect());
    }
    assert_eq!(two.entries().count(), three.entries().count());
    let news = three.active(&news_source()).unwrap();
    assert!(!news.counsel_confirmed(), "counsel has not confirmed it");
    assert_eq!(
        news.approval(),
        Some(&Approval {
            by: Approver::Founder,
            decision: "DEC-2394".to_owned()
        })
    );
    assert!(news.licensed(), "its text stays withheld");
    let edgar = three.active(&edgar_source()).unwrap();
    assert!(edgar.counsel_confirmed());
    assert_eq!(edgar.approval(), None);
    assert!(
        three
            .permit(
                &news_source(),
                "https://data.alpaca.markets/v1beta1/news?symbols=AAPL"
            )
            .is_ok()
    );
    let research = three.research_allowlist();
    assert_eq!(research.version, AllowlistVersion(3));
    assert_eq!(
        research.sources,
        BTreeSet::from([edgar_source(), news_source()])
    );
}

/// DEC-2276: an approver needs the decision that records it, and a decision needs an approver; the
/// decision is a `DEC-<n>` id; the founder is the only approver besides counsel. An entry with
/// neither and no counsel confirmation stays out of effect.
#[test]
fn an_approval_is_the_founder_s_with_its_decision_or_nothing() {
    let entry = |extra: &str| {
        format!(
            "version = 4\n\n[[source]]\nid = \"alpaca.news\"\nkind = \"news\"\nlicensed = true\ncounsel_confirmed = false\n{extra}min_interval_ms = 300\nmax_response_bytes = 1048576\nendpoints = [\"https://data.alpaca.markets/v1beta1/news\"]\n"
        )
    };
    let approved = Allowlist::parse(&entry(
        "approved_by = \"founder\"\napproved_in = \"DEC-2394\"\n",
    ))
    .unwrap();
    assert!(approved.active(&news_source()).is_some());
    let unapproved = Allowlist::parse(&entry("")).unwrap();
    assert!(unapproved.active(&news_source()).is_none());
    assert!(unapproved.research_allowlist().sources.is_empty());
    assert_eq!(
        unapproved.permit(
            &news_source(),
            "https://data.alpaca.markets/v1beta1/news?symbols=AAPL"
        ),
        Err(SourceError::SourceNotAllowlisted)
    );
    for refused in [
        "approved_by = \"founder\"\n",
        "approved_in = \"DEC-2394\"\n",
        "approved_by = \"counsel\"\napproved_in = \"DEC-2394\"\n",
        "approved_by = \"agent\"\napproved_in = \"DEC-2394\"\n",
        "approved_by = \"founder\"\napproved_in = \"2394\"\n",
        "approved_by = \"founder\"\napproved_in = \"DEC-\"\n",
        "approved_by = \"founder\"\napproved_in = \"DEC-02394\"\n",
        "approved_by = \"founder\"\napproved_in = \"DEC-2394a\"\n",
        "approved_by = \"founder\"\napproved_in = \"\"\n",
    ] {
        assert_eq!(
            Allowlist::parse(&entry(refused)),
            Err(SourceError::AllowlistMalformed),
            "{refused}"
        );
    }
}

/// Every approval the checked-in version in use records names a decision that exists and that the
/// founder accepted: the record can cite only an approval that happened (DEC-2276).
#[test]
fn every_recorded_approval_names_a_decision_the_founder_accepted() {
    let three = checked_in(3);
    let mut cited = 0;
    for entry in three.entries() {
        let Some(approval) = entry.approval() else {
            continue;
        };
        assert_eq!(approval.by, Approver::Founder);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../docs/project/decisions/{}.md",
            approval.decision
        ));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "{} cites {}, which has no file",
                entry.id().as_str(),
                approval.decision
            )
        });
        let status = text
            .lines()
            .find(|line| line.starts_with("| **Status** |"))
            .unwrap_or_else(|| panic!("{} has no status", approval.decision));
        assert!(
            status.starts_with("| **Status** | Accepted (founder"),
            "{}: {status}",
            approval.decision
        );
        cited += 1;
    }
    assert_eq!(cited, 1, "Alpaca news, on DEC-2394");
}
