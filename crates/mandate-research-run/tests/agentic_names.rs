//! The two-way name tables of the agentic loop's tools and codes (agent harness spec §6.10, §6.13;
//! journal spec §9.22): each spelling maps back to the one value that produced it.

use std::collections::BTreeSet;

use mandate_research_run::agentic::{Code, Tool};

#[test]
fn every_tool_name_maps_back_to_its_tool_and_names_are_unique() {
    let names: BTreeSet<&str> = Tool::ALL.iter().map(|tool| tool.name()).collect();
    assert_eq!(names.len(), Tool::ALL.len(), "two tools share a name");
    for tool in Tool::ALL {
        assert_eq!(
            Tool::from_name(tool.name()),
            Some(tool),
            "{} does not map back",
            tool.name()
        );
    }
    assert_eq!(
        Tool::from_name("market__bars"),
        None,
        "a wire name is not a declared name"
    );
    assert_eq!(Tool::from_name(""), None);
}

#[test]
fn every_code_spelling_maps_back_to_its_code_and_spellings_are_unique() {
    let spellings: BTreeSet<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
    assert_eq!(
        spellings.len(),
        Code::ALL.len(),
        "two codes share a spelling"
    );
    for code in Code::ALL {
        assert_eq!(
            Code::from_spelling(code.as_str()),
            Some(code),
            "{} does not map back",
            code.as_str()
        );
    }
    assert_eq!(
        Code::from_spelling("admitted"),
        None,
        "the model is never told admitted"
    );
    assert_eq!(
        Code::from_spelling("cooldown"),
        None,
        "a gate reason is never a model code"
    );
}
