//! An agentic research entry's pinned loop (agent harness spec v0.2 §6.9 to §6.11; DEC-2262;
//! backlog E19-13 slice 3c): `final_output`, only the read and think tools the worker serves, on
//! both run kinds' menus, the pinned bounds, and each kind's output schema; no schema an order,
//! a quantity, a size, or a price could be written in; and a loop the registry would refuse is
//! refused here first.

mod common;

use std::collections::BTreeSet;

use common::{entry, registry};
use mandate_canon::{Key, Value, parse};
use mandate_gateway::registry::{Mode, PinnedContent};
use mandate_gateway::schema::fits;
use mandate_gateway::tools::{Agentic, LoopBounds as PinnedBounds, OutputKind, ToolClass};
use mandate_research_run::agentic::LoopBounds;
use mandate_research_run::{OutputBounds, ResearchRunError, agentic_loop, output_schema};

/// The tools the worker's dispatch serves (slice 3b) and the two think tools, by name.
const SERVED: [&str; 8] = [
    "filings.get",
    "filings.search",
    "market.bars",
    "market.quote",
    "news.get",
    "news.search",
    "note",
    "plan",
];

/// Names an order, a quantity, a size, or a price to trade at would be written under (rule 4).
const TRADING: [&str; 16] = [
    "amount",
    "limit_price",
    "notional",
    "order",
    "order_type",
    "price",
    "qty",
    "quantity",
    "shares",
    "side",
    "size",
    "stop_price",
    "target_price",
    "time_in_force",
    "units",
    "weight",
];

fn json(text: &str) -> Value {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e:?}"))
}

fn pinned() -> Agentic {
    agentic_loop(&LoopBounds::v0_2(), &OutputBounds::v0_1()).unwrap_or_else(|e| panic!("{e:?}"))
}

/// Every member name any schema in `schema` declares, at any depth.
fn member_names(schema: &Value, names: &mut BTreeSet<String>) {
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, inner) in properties {
            names.insert(name.as_str().to_owned());
            member_names(inner, names);
        }
    }
    if let Some(items) = schema.get("items") {
        member_names(items, names);
    }
}

fn arguments(agentic: &Agentic, tool: &str) -> Value {
    agentic
        .tool(tool)
        .unwrap_or_else(|| panic!("{tool} is declared"))
        .arguments
        .clone()
}

#[test]
fn an_unpinnable_loop_has_its_own_code() {
    assert_eq!(ResearchRunError::UnpinnableLoop.code(), "unpinnable_loop");
}

#[test]
fn the_loop_is_final_output_with_the_pinned_bounds_and_registers() {
    let agentic = pinned();
    assert_eq!(agentic.output, OutputKind::FinalOutput);
    assert_eq!(
        agentic.loop_bounds,
        PinnedBounds {
            max_turns: 8,
            max_tool_calls: 24,
            max_tool_calls_per_turn: 4,
            max_theses: 4,
            max_result_bytes: 24_000,
            max_run_result_bytes: 96_000,
            max_repeat_denials: 2,
            max_run_s: 900,
            tool_timeout_s: 10,
        },
        "§6.11's defaults, as the gateway pins them"
    );
    assert!(agentic.well_formed());
    let (mut registry, _) = registry();
    let content = PinnedContent {
        model_version: "0.2.0".to_owned(),
        mode: Mode::Agentic(agentic),
        ..entry().content()
    };
    assert!(registry.register(content).is_ok(), "the registry pins it");
}

#[test]
fn only_the_served_read_and_think_tools_are_declared_on_both_menus() {
    let agentic = pinned();
    let names: Vec<&str> = agentic.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names, SERVED,
        "strictly sorted, and nothing the worker cannot serve"
    );
    for tool in &agentic.tools {
        assert_ne!(
            tool.class,
            ToolClass::Action,
            "{}: no action is declared (rule 4)",
            tool.name
        );
        let think = matches!(tool.name.as_str(), "note" | "plan");
        let class = if think {
            ToolClass::Think
        } else {
            ToolClass::Read
        };
        assert_eq!(tool.class, class, "{}", tool.name);
        assert_eq!(tool.menu, ["ideate", "monitor"], "{}", tool.name);
        assert_eq!(tool.result_cap_bytes, 24_000, "{}", tool.name);
        assert!(tool.well_formed(), "{}", tool.name);
    }
}

#[test]
fn no_schema_names_an_order_a_quantity_a_size_or_a_price() {
    let agentic = pinned();
    let mut names = BTreeSet::new();
    for tool in &agentic.tools {
        member_names(&tool.arguments, &mut names);
    }
    for schema in agentic.output_schemas.values() {
        member_names(schema, &mut names);
    }
    for expected in [
        "symbol",
        "symbols",
        "text",
        "theses",
        "invalidations",
        "thesis_id",
    ] {
        assert!(
            names.contains(expected),
            "the walk reads {expected}: {names:?}"
        );
    }
    let found: Vec<&str> = TRADING
        .iter()
        .copied()
        .filter(|name| names.contains(*name))
        .collect();
    assert!(found.is_empty(), "trading members: {found:?}");
}

#[test]
fn ideate_is_the_single_call_s_schema_and_monitor_has_no_theses() {
    let agentic = pinned();
    let kinds: Vec<&str> = agentic.output_schemas.keys().map(String::as_str).collect();
    assert_eq!(kinds, ["ideate", "monitor"]);
    let ideate = &agentic.output_schemas["ideate"];
    assert_eq!(*ideate, output_schema(&OutputBounds::v0_1()));
    let monitor = &agentic.output_schemas["monitor"];
    let members: Vec<&str> = monitor
        .get("properties")
        .and_then(Value::as_object)
        .map(|members| members.keys().map(Key::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        members,
        ["invalidations"],
        "only risk-reducing output (§6.9)"
    );
    assert_eq!(
        monitor
            .get("properties")
            .and_then(|p| p.get("invalidations")),
        ideate
            .get("properties")
            .and_then(|p| p.get("invalidations")),
        "the same invalidations as an ideate run's"
    );
}

#[test]
fn each_tool_s_schema_admits_the_arguments_the_dispatch_reads_and_no_more() {
    let agentic = pinned();
    for (tool, admitted, refused) in [
        (
            "market.bars",
            r#"{"count":260,"symbol":"AAPL"}"#,
            r#"{"count":261,"symbol":"AAPL"}"#,
        ),
        (
            "market.quote",
            r#"{"symbols":["AAPL","MSFT"]}"#,
            r#"{"symbols":["A","B","C","D","E","F","G","H","I","J","K"]}"#,
        ),
        (
            "news.search",
            r#"{"limit":20,"symbols":["AAPL"],"terms":["services"],"window_h":168}"#,
            r#"{"limit":20,"symbols":["AAPL"],"terms":[],"window_h":169}"#,
        ),
        (
            "news.get",
            r#"{"item":"n-1"}"#,
            r#"{"item":"n-1","text":"more"}"#,
        ),
        (
            "filings.search",
            r#"{"forms":["10-Q","8-K"],"limit":10,"symbols":["AAPL"],"terms":[],"window_d":90}"#,
            r#"{"forms":["S-1"],"limit":10,"symbols":["AAPL"],"terms":[],"window_d":90}"#,
        ),
        (
            "filings.get",
            r#"{"item":"f-1","length":8000,"offset":0}"#,
            r#"{"item":"f-1","length":8001,"offset":0}"#,
        ),
        ("note", r#"{"text":"check the 10-Q"}"#, r#"{"text":7}"#),
        (
            "plan",
            r#"{"steps":["read the 10-Q","compare the guidance"]}"#,
            r#"{"steps":["1","2","3","4","5","6"]}"#,
        ),
    ] {
        let schema = arguments(&agentic, tool);
        assert!(fits(&schema, &json(admitted)), "{tool} admits {admitted}");
        assert!(!fits(&schema, &json(refused)), "{tool} refuses {refused}");
    }
}

#[test]
fn bounds_the_registry_would_refuse_are_refused() {
    let output = OutputBounds::v0_1();
    for broken in [
        LoopBounds {
            max_turns: 0,
            ..LoopBounds::v0_2()
        },
        LoopBounds {
            max_tool_calls_per_turn: 25,
            ..LoopBounds::v0_2()
        },
        LoopBounds {
            max_result_bytes: 96_001,
            ..LoopBounds::v0_2()
        },
    ] {
        assert_eq!(
            agentic_loop(&broken, &output).map(|_| ()),
            Err(ResearchRunError::UnpinnableLoop),
            "{broken:?}"
        );
    }
}
