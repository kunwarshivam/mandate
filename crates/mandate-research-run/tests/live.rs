//! The live registry file (DEC-2282 item 6, DEC-2326 item 1): the research role's vetted entries,
//! each registered, probed on its one endpoint, and offered only as the file and the gateway's
//! registry both allow.

use mandate_canon::{Value, parse};
use mandate_gateway::registry::{Endpoint, RegistryError, Reported, Status, Unresolved};
use mandate_research_run::live::{Refused, live_registry, read_live};
use mandate_research_run::recorded::RecordedError;

fn entry(id: &str, provider: &str, slug: &str, snapshot: &str, status: &str) -> String {
    format!(
        r#"{{"status":"{status}",
            "entry":{{"agentic":"v0_2","deadline_ms":120000,"max_output_tokens":4096,"model_id":"{id}",
                      "model_name":"{slug}","model_version":"1.0.0","provider":"{provider}","seed":7,
                      "snapshot":"{snapshot}"}},
            "endpoint":{{"cached_per_million":"0.1","host":"openrouter.ai","id":"openrouter:{id}",
                         "input_per_million":"2","output_per_million":"10","price_table":"openrouter 2026-10-27",
                         "region":"global","reported_provider":"Google","upstream":"google-vertex/global"}}}}"#
    )
}

fn file(entries: &[String]) -> Value {
    parse(format!(r#"{{"entries":[{}]}}"#, entries.join(",")).as_bytes()).unwrap()
}

fn vetted() -> Value {
    file(&[
        entry(
            "llm.research_sonnet",
            "anthropic",
            "anthropic/claude-sonnet-5.5",
            "claude-sonnet-5-5",
            "offered",
        ),
        entry(
            "llm.research_haiku",
            "anthropic",
            "anthropic/claude-haiku-5.5",
            "claude-haiku-5-5",
            "offered",
        ),
        entry(
            "llm.research_terra",
            "openai",
            "openai/gpt-5.6-terra",
            "gpt-5.6-terra",
            "offered",
        ),
    ])
}

/// The probe an endpoint answers with what it was registered to serve.
fn honest(endpoint: &Endpoint, slug: &str) -> Vec<Reported> {
    vec![Reported {
        model: Some(slug.to_owned()),
        provider: Some(endpoint.route.reported_provider.clone()),
    }]
}

#[test]
fn the_vetted_list_registers_a_and_b_and_refuses_an_undated_c() {
    let entries = read_live(&vetted()).unwrap();
    let mut probed = Vec::new();
    let (registry, refused) = live_registry(&entries, &mut |endpoint, content| {
        probed.push(content.model_id.clone());
        honest(endpoint, &content.identity.model_name)
    });
    assert_eq!(
        probed,
        ["llm.research_sonnet", "llm.research_haiku"],
        "an entry the registry refuses is never probed, so a live probe spends nothing on it"
    );
    for live in &entries[..2] {
        let pin = live.entry.content().model_ref();
        assert_eq!(
            registry.resolve(&pin).map(|e| e.status()),
            Ok(Status::Offered),
            "{} is offered",
            pin.model_id
        );
    }
    assert_eq!(
        refused,
        vec![Refused {
            model_id: "llm.research_terra".to_owned(),
            error: RegistryError::FloatingAlias,
        }],
        "C is refused at registration while OpenAI dates no snapshot (DEC-2282 item 7)"
    );
    let terra = entries[2].entry.content().model_ref();
    assert_eq!(
        registry.resolve(&terra).err(),
        Some(Unresolved::Unknown),
        "no pin can name C"
    );
}

#[test]
fn an_endpoint_that_fails_its_probe_leaves_its_entry_unoffered() {
    let entries = read_live(&vetted()).unwrap();
    let (registry, refused) = live_registry(&entries, &mut |endpoint, content| {
        if content.model_id == "llm.research_haiku" {
            honest(endpoint, "anthropic/claude-sonnet-5.5")
        } else {
            honest(endpoint, &content.identity.model_name)
        }
    });
    let haiku = entries[1].entry.content().model_ref();
    assert!(refused.contains(&Refused {
        model_id: "llm.research_haiku".to_owned(),
        error: RegistryError::ProbeFailed,
    }));
    let held = registry.resolve(&haiku).unwrap();
    assert_eq!(
        held.status(),
        Status::Evaluating,
        "a failed probe puts the entry back to evaluating"
    );
    assert_eq!(held.endpoints().count(), 0, "and adds no endpoint");
    let sonnet = entries[0].entry.content().model_ref();
    assert_eq!(
        registry.resolve(&sonnet).map(|e| e.status()),
        Ok(Status::Offered)
    );
}

#[test]
fn an_entry_the_file_holds_evaluating_is_registered_but_not_offered() {
    let entries = read_live(&file(&[entry(
        "llm.research_sonnet",
        "anthropic",
        "anthropic/claude-sonnet-5.5",
        "claude-sonnet-5-5",
        "evaluating",
    )]))
    .unwrap();
    let (registry, refused) = live_registry(&entries, &mut |endpoint, content| {
        honest(endpoint, &content.identity.model_name)
    });
    assert!(refused.is_empty());
    let pin = entries[0].entry.content().model_ref();
    let held = registry.resolve(&pin).unwrap();
    assert_eq!(held.status(), Status::Evaluating);
    assert_eq!(
        held.endpoints().count(),
        0,
        "an entry not offered holds no endpoint, so no call reaches it"
    );
}

#[test]
fn an_entry_with_other_harness_content_is_refused_and_holds_no_endpoint() {
    let mut entries = read_live(&vetted()).unwrap();
    entries.truncate(2);
    let mut wide = entries[1].clone();
    wide.entry.model_id = "llm.research_wide".to_owned();
    if let Some(bounds) = wide.entry.agentic.as_mut() {
        bounds.max_turns = 20;
    }
    entries.push(wide);
    let mut probed = Vec::new();
    let (registry, refused) = live_registry(&entries, &mut |endpoint, content| {
        probed.push(content.model_id.clone());
        honest(endpoint, &content.identity.model_name)
    });
    assert_eq!(
        refused,
        vec![Refused {
            model_id: "llm.research_wide".to_owned(),
            error: RegistryError::HarnessMismatch,
        }]
    );
    let held = registry
        .resolve(&entries[2].entry.content().model_ref())
        .unwrap();
    assert_eq!(held.status(), Status::Evaluating);
    assert_eq!(
        held.endpoints().count(),
        0,
        "a refused offer adds no endpoint"
    );
    assert!(
        !probed.iter().any(|id| id == "llm.research_wide"),
        "and spends no probe"
    );
}

#[test]
fn a_malformed_file_is_refused_whole() {
    let good = entry(
        "llm.research_sonnet",
        "anthropic",
        "anthropic/claude-sonnet-5.5",
        "claude-sonnet-5-5",
        "offered",
    );
    for (text, member) in [
        (r#"{}"#.to_owned(), "entries"),
        (r#"{"entries":[]}"#.to_owned(), "entries"),
        (good.replace(r#""status":"offered","#, ""), "status"),
        (
            good.replace(r#""status":"offered""#, r#""status":"deprecated""#),
            "status",
        ),
        (
            good.replace(r#""upstream":"google-vertex/global""#, r#""upstream":"""#),
            "upstream",
        ),
    ] {
        let value = if text.contains("\"entry\"") {
            file(&[text])
        } else {
            parse(text.as_bytes()).unwrap()
        };
        assert_eq!(
            read_live(&value).err(),
            Some(RecordedError::Malformed(member)),
            "{member}"
        );
    }
}

/// DEC-2216 item 4: an entry may name its passing gate's report by reference; absent, it names
/// none, and a reference that is not one refuses the whole file.
#[test]
fn an_entry_may_name_its_gate_report() {
    let good = entry(
        "llm.research_sonnet",
        "anthropic",
        "anthropic/claude-sonnet-5.5",
        "claude-sonnet-5-5",
        "offered",
    );
    let reference = format!("sha256:{}", "a".repeat(64));
    let named = good.replacen(
        r#""status":"offered","#,
        &format!(r#""status":"offered","gate_report":"{reference}","#),
        1,
    );
    let entries = read_live(&file(std::slice::from_ref(&named))).unwrap();
    assert_eq!(entries[0].gate_report.as_deref(), Some(reference.as_str()));
    assert_eq!(read_live(&file(&[good])).unwrap()[0].gate_report, None);
    let upper = format!("sha256:{}", "A".repeat(64));
    let not_hex = format!("sha256:{}", "g".repeat(64));
    let long = format!("sha256:{}", "a".repeat(65));
    for bad in [
        "sha256:short",
        "a".repeat(71).as_str(),
        "",
        &upper,
        &not_hex,
        &long,
    ] {
        let text = named.replace(&reference, bad);
        assert_eq!(
            read_live(&file(&[text])).err(),
            Some(RecordedError::Malformed("gate_report")),
            "{bad:?}"
        );
    }
}
