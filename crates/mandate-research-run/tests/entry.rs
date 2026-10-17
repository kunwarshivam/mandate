//! The research entry (inference spec §4.1, agent harness spec §6.4, §6.5): its template takes the
//! three typed inputs and nothing else, its output schema is §6.5's in the gateway's language, and
//! every part is inside the content hash, so a change is a new pin.

mod common;

use common::{endpoint, entry, registry};
use mandate_canon::{Value, parse};
use mandate_gateway::registry::{Registry, Reported};
use mandate_gateway::schema::{fits, well_formed};
use mandate_research_run::{OutputBounds, RESEARCH_INPUTS, RetrievalPlan, output_schema};

fn json(text: &str) -> Value {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text} is strict JSON: {e:?}"))
}

fn thesis(horizon: u64) -> String {
    format!(
        r#"{{"confidence":"0.6","conviction":"0.7","direction":"long","evidence":["n-1"],"horizon_s":{horizon},"instrument":"AAPL","invalidation":"Guidance is cut.","thesis":"Services revenue keeps growing."}}"#
    )
}

#[test]
fn the_entry_registers_with_its_template_s_three_slots() {
    let content = entry().content();
    assert_eq!(
        content.template.inputs,
        RESEARCH_INPUTS
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>()
    );
    for slot in RESEARCH_INPUTS {
        assert!(
            content.template.user.contains(&format!("{{{{{slot}}}}}")),
            "the user text has the {slot} slot"
        );
    }
    assert!(
        !content.template.system.contains("{{"),
        "the system text has no slot"
    );
    assert!(
        content.template.system.contains("untrusted"),
        "the system text says data sections are untrusted (§6.4)"
    );
    assert_eq!(content.output_schema, output_schema(&OutputBounds::v0_1()));
    let mut fresh = Registry::new();
    assert_eq!(
        fresh.register(content.clone()),
        Ok(content.content_hash()),
        "the registry's checks pass"
    );
    assert_eq!(registry().1, content.model_ref());
    let probe = Reported {
        model: Some(common::SLUG.to_owned()),
        provider: Some(common::REPORTED_PROVIDER.to_owned()),
    };
    assert_eq!(
        fresh.add_endpoint(&content.model_ref(), endpoint(), &[probe]),
        Ok(())
    );
}

#[test]
fn the_schema_is_section_6_5_s_and_well_formed() {
    let schema = output_schema(&OutputBounds::v0_1());
    assert!(well_formed(&schema));
    let good = json(&format!(
        r#"{{"invalidations":[{{"evidence":["n-2"],"thesis_id":"01T"}}],"theses":[{}]}}"#,
        thesis(86_400)
    ));
    assert!(fits(&schema, &good));
    let empty = json(r#"{"invalidations":[],"theses":[]}"#);
    assert!(fits(&schema, &empty));
}

#[test]
fn the_schema_refuses_what_section_6_5_bounds() {
    let schema = output_schema(&OutputBounds::v0_1());
    let output = |theses: &str| json(&format!(r#"{{"invalidations":[],"theses":[{theses}]}}"#));
    assert!(!fits(&schema, &output(&thesis(86_399))), "under a day");
    assert!(fits(&schema, &output(&thesis(2_592_000))), "thirty days");
    assert!(
        !fits(&schema, &output(&thesis(2_592_001))),
        "over thirty days"
    );
    let four = [
        thesis(86_400),
        thesis(86_400),
        thesis(86_400),
        thesis(86_400),
    ]
    .join(",");
    assert!(fits(&schema, &output(&four)), "four theses");
    let five = [four.clone(), thesis(86_400)].join(",");
    assert!(!fits(&schema, &output(&five)), "five theses");
    let revision =
        thesis(86_400).replace(r#""thesis":"#, r#""predecessor_thesis_id":"01T","thesis":"#);
    assert!(
        !fits(&schema, &output(&revision)),
        "a revision is refused while §8.6 item 5 allows none (DEC-1120 item 3)"
    );
    let extra = thesis(86_400).replace(r#""thesis":"#, r#""order":"buy","thesis":"#);
    assert!(!fits(&schema, &output(&extra)), "no member beyond §6.5's");
    let number = thesis(86_400).replace(r#""conviction":"0.7""#, r#""conviction":1"#);
    assert!(
        !fits(&schema, &output(&number)),
        "conviction is decimal text"
    );
    let long = thesis(86_400).replace("Services revenue keeps growing.", &"x".repeat(2_001));
    assert!(!fits(&schema, &output(&long)), "thesis text over its bound");
    let evidence: Vec<String> = (0..21).map(|n| format!(r#""n-{n}""#)).collect();
    let many = thesis(86_400).replace(r#"["n-1"]"#, &format!("[{}]", evidence.join(",")));
    assert!(
        !fits(&schema, &output(&many)),
        "more evidence than the bound"
    );
    let invalidations: Vec<String> = (0..51)
        .map(|n| format!(r#"{{"evidence":[],"thesis_id":"t{n}"}}"#))
        .collect();
    let flood = json(&format!(
        r#"{{"invalidations":[{}],"theses":[]}}"#,
        invalidations.join(",")
    ));
    assert!(!fits(&schema, &flood), "more invalidations than the bound");
}

#[test]
fn every_part_of_the_entry_is_inside_the_hash() {
    let base = entry().content().content_hash();
    let mut changes = Vec::new();
    let mut e = entry();
    e.plan = RetrievalPlan {
        news_max_items: 49,
        ..RetrievalPlan::v0_1()
    };
    changes.push(("plan", e));
    let mut e = entry();
    e.bounds = OutputBounds {
        max_theses: 3,
        ..OutputBounds::v0_1()
    };
    changes.push(("bounds", e));
    let mut e = entry();
    e.deadline_ms = 60_000;
    changes.push(("deadline", e));
    let mut e = entry();
    e.max_output_tokens = 2_048;
    changes.push(("tokens", e));
    let mut e = entry();
    e.seed = None;
    changes.push(("seed", e));
    let mut e = entry();
    e.identity.snapshot =
        mandate_gateway::registry::Snapshot::Dated("claude-sonnet-4-5-20251001".to_owned());
    changes.push(("identity", e));
    for (what, changed) in changes {
        assert_ne!(changed.content().content_hash(), base, "{what}");
    }
    let content = entry().content();
    assert_eq!(content.model_id, "llm.research_agent");
    assert_eq!(content.model_version, "0.1.0");
    assert_eq!(content.deadline_ms, 120_000);
    assert_eq!(content.max_output_tokens, 4_096);
    assert_eq!(content.seed, Some(7));
    assert!(!content.methodology.is_empty() && !content.authorship.is_empty());
    let bounds = &content.validation_bounds;
    assert_eq!(bounds.get("max_theses").and_then(Value::as_int), Some(4));
    assert_eq!(
        bounds.get("max_output_bytes").and_then(Value::as_int),
        Some(65_536)
    );
    let plan = &content.retrieval_plan;
    assert_eq!(
        plan.get("bars_per_instrument").and_then(Value::as_int),
        Some(20)
    );
    assert_eq!(
        plan.get("retrieval_timeout_s").and_then(Value::as_int),
        Some(60)
    );
}
