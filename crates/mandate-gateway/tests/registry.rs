//! The model registry (inference spec §4, E15-7): the content hash covers every pinned member and
//! nothing outside them, floating aliases are refused, an endpoint needs the identity probe, and
//! withdrawal is final.

mod common;

use std::collections::BTreeMap;

use common::*;
use mandate_canon::Digest;
use mandate_gateway::registry::{
    FAST_DEADLINE_MS, Identity, Mode, PinnedContent, Registry, RegistryError, Reported,
    SLOW_DEADLINE_MS, Snapshot, Status, Unresolved,
};
use mandate_time::Date;

#[test]
fn every_pinned_member_changes_the_content_hash() {
    let base = content().content_hash();
    let mut changed: Vec<(&str, mandate_gateway::registry::PinnedContent)> = Vec::new();
    let mut c = content();
    c.model_id = "llm.research_agent_b".to_owned();
    changed.push(("model_id", c));
    let mut c = content();
    c.model_version = "0.1.1".to_owned();
    changed.push(("model_version", c));
    let mut c = content();
    c.identity.provider = "other".to_owned();
    changed.push(("identity.provider", c));
    let mut c = content();
    c.identity.model_name = "anthropic/claude-opus-4.1".to_owned();
    changed.push(("identity.model_name", c));
    let mut c = content();
    c.identity.snapshot = Snapshot::Dated("claude-sonnet-4-5-20250930".to_owned());
    changed.push(("identity.snapshot", c));
    let mut c = content();
    c.identity.quantization = Some("fp8".to_owned());
    changed.push(("identity.quantization", c));
    let mut c = content();
    c.template.system.push('!');
    changed.push(("template.system", c));
    let mut c = content();
    c.template.user.push('!');
    changed.push(("template.user", c));
    let mut c = content();
    c.retrieval_plan = json(r#"{"bars":{"days":31},"filings":{"max_items":5}}"#);
    changed.push(("retrieval_plan", c));
    let mut c = content();
    c.output_schema = json(r#"{"type":"boolean"}"#);
    changed.push(("output_schema", c));
    let mut c = content();
    c.validation_bounds = json(r#"{"max_theses":2}"#);
    changed.push(("validation_bounds", c));
    let mut c = content();
    c.params_schema = json(r#"{"k":{"type":"integer","minimum":0,"maximum":1}}"#);
    changed.push(("params_schema", c));
    let mut c = content();
    c.deadline_ms = 59_999;
    changed.push(("deadline_ms", c));
    let mut c = content();
    c.max_output_tokens = 999;
    changed.push(("max_output_tokens", c));
    let mut c = content();
    c.seed = None;
    changed.push(("seed", c));
    let mut c = content();
    c.methodology.push('!');
    changed.push(("methodology", c));
    let mut c = content();
    c.authorship = "partner".to_owned();
    changed.push(("authorship", c));
    let mut c = content();
    c.template.inputs = vec!["bars".to_owned(), "filings".to_owned(), "news".to_owned()];
    c.template.user.push_str("{{news}}");
    changed.push(("template.inputs", c));
    let mut seen = std::collections::BTreeSet::from([base]);
    for (member, c) in changed {
        assert!(
            seen.insert(c.content_hash()),
            "changing {member} must change the hash"
        );
    }
}

#[test]
fn endpoints_and_status_are_outside_the_hash() {
    let (mut registry, model) = registry();
    let before = registry.resolve(&model).unwrap().content_hash();
    let mut second = endpoint();
    second.id = "openrouter:anthropic-eu".to_owned();
    second.region = "eu".to_owned();
    registry
        .add_endpoint(&model, second, &[reported()])
        .unwrap();
    registry
        .set_status(
            &model,
            Status::Deprecated {
                retires_on: Date::new(2027, 1, 31).unwrap(),
            },
        )
        .unwrap();
    registry
        .set_healthy(&model, "openrouter:anthropic", false)
        .unwrap();
    let entry = registry.resolve(&model).unwrap();
    assert_eq!(entry.content_hash(), before);
    assert_eq!(entry.content_hash(), content().content_hash());
    assert_eq!(entry.endpoints().count(), 2);
    assert_eq!(model.content_hash, before, "the owner's pin still resolves");
}

#[test]
fn the_content_hash_is_sha256_of_the_canonical_pinned_content() {
    let c = content();
    assert_eq!(
        c.content_hash(),
        Digest::of(&mandate_canon::to_canonical(&c.canonical()))
    );
    let canonical = String::from_utf8(mandate_canon::to_canonical(&c.canonical())).unwrap();
    assert!(canonical.starts_with(r#"{"authorship":"platform","deadline_ms":60000,"identity":"#));
    assert!(canonical.contains(r#""sampling":{"seed":7,"temperature":"0"}"#));
    assert!(!canonical.contains("endpoint") && !canonical.contains("status"));
}

#[test]
fn floating_aliases_variants_and_undated_snapshots_are_refused_at_registration() {
    let names = [
        "",
        "openrouter/auto",
        "anthropic/claude-sonnet-latest",
        "anthropic/claude-sonnet-4.5:nitro",
        "anthropic/claude-sonnet-4.5:online",
        "anthropic/claude-sonnet-4.5:free",
        "~anthropic/claude-sonnet",
        "meta/llama-default",
        "x/stable",
    ];
    for name in names {
        let mut c = content();
        c.identity.model_name = name.to_owned();
        assert_eq!(
            Registry::new().register(c).err(),
            Some(RegistryError::FloatingAlias),
            "{name:?}"
        );
    }
    let snapshots = [
        "claude-",
        "claude-sonnet-5.5",
        "claude-sonnet-latest",
        "latest",
        "2025-13-40",
        "claude-20250229-latest",
        "v20251",
        "build-99999999",
        "2025-0-929",
        "2025--9-29",
    ];
    for snapshot in snapshots {
        let mut c = content();
        c.identity.snapshot = Snapshot::Dated(snapshot.to_owned());
        assert_eq!(
            Registry::new().register(c).err(),
            Some(RegistryError::FloatingAlias),
            "{snapshot:?}"
        );
    }
    for (provider, snapshot) in [
        ("openai", "gpt-5.6-terra"),
        ("google", "gemini-3-pro"),
        ("openai", "claude-sonnet-5-5"),
    ] {
        let mut c = content();
        c.identity.provider = provider.to_owned();
        c.identity.snapshot = Snapshot::Dated(snapshot.to_owned());
        assert_eq!(
            Registry::new().register(c).err(),
            Some(RegistryError::FloatingAlias),
            "{provider} states no dateless id is pinned, so {snapshot:?} is refused"
        );
    }
    for snapshot in ["claude-sonnet-5-5", "claude-haiku-5-5"] {
        let mut c = content();
        c.identity.snapshot = Snapshot::Dated(snapshot.to_owned());
        assert!(
            Registry::new().register(c).is_ok(),
            "{snapshot:?}: Anthropic states every model id is a pinned snapshot (DEC-2282 item 7)"
        );
    }
    let mut c = content();
    c.identity.provider = String::new();
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::FloatingAlias)
    );
    for snapshot in [
        "claude-sonnet-4-5-20250929",
        "2025-09-29",
        "gpt-4o-2024-08-06",
        "20240229",
    ] {
        let mut c = content();
        c.identity.snapshot = Snapshot::Dated(snapshot.to_owned());
        assert!(Registry::new().register(c).is_ok(), "{snapshot:?} is dated");
    }
    let mut c = content();
    c.identity = Identity {
        provider: "self".to_owned(),
        model_name: "open-weights/model-8b".to_owned(),
        snapshot: Snapshot::Weights(Digest::of(b"weights")),
        quantization: Some("q4_k_m".to_owned()),
    };
    assert!(
        Registry::new().register(c).is_ok(),
        "a weights digest pins exactly"
    );
}

#[test]
fn deadlines_are_finite_and_within_the_tiers_bound() {
    for (id, ms, ok) in [
        ("llm.research_agent", 0, false),
        ("llm.research_agent", SLOW_DEADLINE_MS, true),
        ("llm.research_agent", SLOW_DEADLINE_MS + 1, false),
        ("fast.decider", FAST_DEADLINE_MS, true),
        ("fast.decider", FAST_DEADLINE_MS + 1, false),
        ("fast.decider", 1, true),
    ] {
        let mut c = content();
        c.model_id = id.to_owned();
        c.deadline_ms = ms;
        assert_eq!(Registry::new().register(c).is_ok(), ok, "{id} {ms}");
        if !ok {
            let mut c = content();
            c.model_id = id.to_owned();
            c.deadline_ms = ms;
            assert_eq!(
                Registry::new().register(c).err(),
                Some(RegistryError::Deadline)
            );
        }
    }
}

#[test]
fn an_entry_needs_an_output_bound_a_schema_and_a_matching_template() {
    let mut c = content();
    c.max_output_tokens = 0;
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::NoOutputBound)
    );
    for schema in [
        r#"{"type":"object"}"#,
        r#"{"type":"number"}"#,
        r#"{"type":"string"}"#,
        r#"{"type":"string","max_length":3,"enum":["a"]}"#,
        r#"{"type":"string","enum":[]}"#,
        r#"{"type":"string","enum":[1]}"#,
        r#"{"type":"integer","minimum":5,"maximum":4}"#,
        r#"{"type":"integer","minimum":5}"#,
        r#"{"type":"array","items":{"type":"boolean"}}"#,
        r#"{"type":"array","max_items":2}"#,
        r#"{"type":"boolean","extra":1}"#,
        r#"{"type":"object","properties":{"a":{"type":"float"}}}"#,
        r#"{"type":"object","properties":{},"required":[]}"#,
        r#"[]"#,
    ] {
        let mut c = content();
        c.output_schema = json(schema);
        assert_eq!(
            Registry::new().register(c).err(),
            Some(RegistryError::OutputSchema),
            "{schema}"
        );
    }
    let mut c = content();
    c.template.inputs = vec!["filings".to_owned(), "bars".to_owned()];
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::Template),
        "inputs sorted"
    );
    let mut c = content();
    c.template.inputs = vec!["bars".to_owned()];
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::Template),
        "every slot listed"
    );
    let mut c = content();
    c.template.inputs = vec!["bars".to_owned(), "filings".to_owned(), "news".to_owned()];
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::Template),
        "every input has a slot"
    );
    let mut c = content();
    c.template.system.push_str("{{bars}}");
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::Template),
        "no slot in the system text"
    );
}

#[test]
fn pinned_content_is_immutable_once_registered() {
    let mut registry = Registry::new();
    let first = registry.register(content()).unwrap();
    assert_eq!(
        registry.register(content()),
        Ok(first),
        "the same content again is a no-op"
    );
    let mut changed = content();
    changed.methodology.push('!');
    assert_eq!(registry.register(changed), Err(RegistryError::Immutable));
}

#[test]
fn an_endpoint_is_added_only_after_the_identity_probe() {
    let mut registry = Registry::new();
    let model = content().model_ref();
    registry.register(content()).unwrap();
    let wrong = Reported {
        model: Some(SLUG.to_owned()),
        provider: Some("Amazon Bedrock".to_owned()),
    };
    assert_eq!(
        registry.add_endpoint(&model, endpoint(), &[]),
        Err(RegistryError::ProbeFailed)
    );
    assert_eq!(
        registry.add_endpoint(&model, endpoint(), &[reported(), wrong]),
        Err(RegistryError::ProbeFailed)
    );
    assert_eq!(
        registry.add_endpoint(&model, endpoint(), &[Reported::default()]),
        Err(RegistryError::ProbeFailed)
    );
    assert_eq!(registry.resolve(&model).unwrap().endpoints().count(), 0);
    registry
        .add_endpoint(&model, endpoint(), &[reported(), reported()])
        .unwrap();
    assert_eq!(
        registry.add_endpoint(&model, endpoint(), &[reported()]),
        Err(RegistryError::DuplicateEndpoint)
    );
    let mut other = model.clone();
    other.content_hash = Digest::of(b"x");
    assert_eq!(
        registry.add_endpoint(&other, endpoint(), &[reported()]),
        Err(RegistryError::HashMismatch)
    );
    let mut unknown = model.clone();
    unknown.model_id = "llm.unknown".to_owned();
    assert_eq!(
        registry.add_endpoint(&unknown, endpoint(), &[reported()]),
        Err(RegistryError::Unknown)
    );
}

#[test]
fn withdrawal_is_final_and_never_replaces_the_model() {
    let (mut registry, model) = registry();
    registry.set_status(&model, Status::Withdrawn).unwrap();
    assert_eq!(registry.resolve(&model).err(), Some(Unresolved::Withdrawn));
    assert_eq!(
        registry.set_status(&model, Status::Offered),
        Err(RegistryError::Withdrawn)
    );
    assert_eq!(
        registry.add_endpoint(&model, endpoint(), &[reported()]),
        Err(RegistryError::Withdrawn)
    );
    assert_eq!(
        registry.entries().count(),
        1,
        "nothing is registered in its place"
    );
}

#[test]
fn a_pin_resolves_only_with_its_own_hash() {
    let (registry, model) = registry();
    assert!(registry.resolve(&model).is_ok());
    let mut other = model.clone();
    other.content_hash = Digest::of(b"x");
    assert_eq!(
        registry.resolve(&other).err(),
        Some(Unresolved::HashMismatch)
    );
    let mut unknown = model;
    unknown.model_version = "2.0.0".to_owned();
    assert_eq!(registry.resolve(&unknown).err(), Some(Unresolved::Unknown));
}

#[test]
fn reported_identity_is_compared_byte_for_byte() {
    let (registry, model) = registry();
    let entry = registry.resolve(&model).unwrap();
    let ep = endpoint();
    assert!(entry.serves(&ep, &reported()));
    for (m, p) in [
        (
            Some("anthropic/claude-sonnet-4.5 "),
            Some(REPORTED_PROVIDER),
        ),
        (Some(SLUG), Some("anthropic")),
        (None, Some(REPORTED_PROVIDER)),
        (Some(SLUG), None),
    ] {
        let r = Reported {
            model: m.map(str::to_owned),
            provider: p.map(str::to_owned),
        };
        assert!(!entry.serves(&ep, &r), "{m:?} {p:?}");
    }
    assert_eq!(
        reported().text().as_deref(),
        Some("anthropic/claude-sonnet-4.5@Anthropic")
    );
    assert_eq!(Reported::default().text(), None);
    let half = Reported {
        model: None,
        provider: Some("P".to_owned()),
    };
    assert_eq!(half.text().as_deref(), Some("@P"));
}

#[test]
fn open_weights_must_name_their_quantization() {
    let mut c = content();
    c.identity = Identity {
        provider: "self".to_owned(),
        model_name: "open-weights/model-8b".to_owned(),
        snapshot: Snapshot::Weights(Digest::of(b"weights")),
        quantization: None,
    };
    assert_eq!(
        Registry::new().register(c).err(),
        Some(RegistryError::Quantization)
    );
}

/// A research entry like `agentic_content`'s but for another model: its id, identity, sampling,
/// deadline and token limit differ, which are the members a role's entries may differ in.
fn another_model(id: &str) -> PinnedContent {
    let mut content = agentic_content();
    content.model_id = id.to_owned();
    content.identity = Identity {
        provider: "anthropic".to_owned(),
        model_name: "anthropic/claude-haiku-5.5".to_owned(),
        snapshot: Snapshot::Dated("claude-haiku-5-5".to_owned()),
        quantization: None,
    };
    content.seed = Some(11);
    content.top_p = None;
    content.reasoning = None;
    content.deadline_ms = 30_000;
    content.max_output_tokens = 2_000;
    content
}

fn offered(registry: &mut Registry, content: PinnedContent) -> Result<(), RegistryError> {
    let model = content.model_ref();
    registry.register(content).unwrap();
    registry.set_status(&model, Status::Offered)
}

/// DEC-2282 and inference spec §4.2 step 5: the owner picks a research model per run kind, so every
/// offered entry of one research role pins the same harness content. A choice then changes which
/// model answers, never what the agent may read or do.
#[test]
fn a_research_entry_is_offered_only_with_its_roles_harness_content() {
    let (mut registry, _) = agentic_registry();
    assert_eq!(
        offered(&mut registry, another_model("llm.research_light")),
        Ok(()),
        "another model with the role's harness content is offered"
    );
    let edits: [(&str, fn(&mut PinnedContent)); 9] = [
        ("template", |c| c.template.system.push_str(" Be bold.")),
        ("retrieval plan", |c| {
            c.retrieval_plan = json(r#"{"bars":{"days":90},"filings":{"max_items":5}}"#)
        }),
        ("validation bounds", |c| {
            c.validation_bounds = json(r#"{"max_theses":9}"#)
        }),
        ("params schema", |c| {
            c.params_schema = json(r#"{"type":"object"}"#)
        }),
        ("methodology", |c| {
            c.methodology.push_str(" Also reads blogs.")
        }),
        ("authorship", |c| c.authorship = "user".to_owned()),
        ("tools", |c| {
            if let Mode::Agentic(a) = &mut c.mode {
                a.tools.pop();
            }
        }),
        ("loop bounds", |c| {
            if let Mode::Agentic(a) = &mut c.mode {
                a.loop_bounds.max_turns = 20;
            }
        }),
        ("an output schema", |c| {
            if let Mode::Agentic(a) = &mut c.mode {
                a.output_schemas
                    .insert("monitor".to_owned(), output_schema());
            }
        }),
    ];
    for (index, (member, edit)) in edits.into_iter().enumerate() {
        let mut content = another_model(&format!("llm.research_edit_{index}"));
        edit(&mut content);
        let model = content.model_ref();
        assert_eq!(
            offered(&mut registry, content),
            Err(RegistryError::HarnessMismatch),
            "an entry whose {member} differs from the offered role's is not offered"
        );
        assert_eq!(
            registry.resolve(&model).map(|e| e.status()),
            Ok(Status::Evaluating),
            "a refused offer leaves the entry evaluating, so no pin may name it ({member})"
        );
    }
}

/// The check binds among entries offered for the same run kinds only: a single-call model, and an
/// entry of another role, are offered beside the research role, and a role with nothing offered
/// takes its first entry as it is.
#[test]
fn the_harness_check_binds_only_within_a_role() {
    let (mut registry, research) = agentic_registry();
    let mut desk = another_model("llm.desk");
    if let Mode::Agentic(a) = &mut desk.mode {
        a.output_schemas = BTreeMap::from([("owner".to_owned(), output_schema())]);
        for tool in &mut a.tools {
            tool.menu = vec!["owner".to_owned()];
        }
    }
    assert_eq!(
        offered(&mut registry, desk),
        Ok(()),
        "another role is offered"
    );
    registry.set_status(&research, Status::Withdrawn).unwrap();
    let mut changed = another_model("llm.research_next");
    changed.methodology.push_str(" A new harness.");
    assert_eq!(
        offered(&mut registry, changed),
        Ok(()),
        "with no entry of the role offered, the next one sets the role's harness"
    );
}
