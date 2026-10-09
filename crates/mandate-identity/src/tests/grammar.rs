//! Identity spec §4.2's matrix, parsed by its grammar (DEC-641, DEC-816) from the spec itself, so
//! no test takes its answers from the code. `matrix.rs` reads it for ID-2's exhaustive test.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Permission, StepUp};

use super::rows::{OWED, RISK_REDUCING, ROLE_COLUMNS, ROWS};

const SPEC: &str = include_str!("../../../../docs/specs/identity.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Blank,
    Grant,
    OrgOnly,
    SelfOnly,
}

pub(crate) struct Row {
    pub(crate) permission: Permission,
    pub(crate) step_up: StepUp,
    pub(crate) inactive: bool,
    pub(crate) reduction: bool,
    pub(crate) risk_reducing: bool,
    pub(crate) self_row: bool,
    pub(crate) own_row: bool,
    cells: BTreeMap<String, Cell>,
}

impl Row {
    pub(crate) fn grants(&self, column: &str, org_scope: bool) -> bool {
        match self.cells.get(column).copied().unwrap_or(Cell::Blank) {
            Cell::Blank | Cell::SelfOnly => false,
            Cell::Grant => true,
            Cell::OrgOnly => org_scope,
        }
    }
}

fn cell(text: &str) -> Cell {
    match text {
        "" => Cell::Blank,
        "✓" | "own" | "✓ (propose only)" | "✓ (not owner)" => Cell::Grant,
        "✓ (org)" => Cell::OrgOnly,
        "self" => Cell::SelfOnly,
        other => panic!("§4.2 cell {other:?} is outside DEC-641's grammar"),
    }
}

fn step_up(text: &str) -> StepUp {
    match text {
        "" => StepUp::NotRequired,
        "S" => StepUp::Required,
        "S for invite" => StepUp::ForInvite,
        "S for grant" => StepUp::ForGrant,
        other => panic!("§4.2 S cell {other:?} is outside DEC-641's grammar"),
    }
}

/// The rows of §4.2, from the section's one table.
pub(crate) fn matrix() -> Vec<Row> {
    let section = SPEC
        .split("### 4.2 Permission matrix")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("identity spec §4.2");
    let mut lines = section.lines().filter(|l| l.starts_with('|'));
    let split = |l: &str| -> Vec<String> {
        let inner = l.trim().trim_start_matches('|').trim_end_matches('|');
        inner.split('|').map(|c| c.trim().to_owned()).collect()
    };
    let header = split(lines.next().expect("the matrix header"));
    assert_eq!(
        header.get(..2),
        Some(&["Permission".to_owned(), "S".to_owned()][..])
    );
    let members: Vec<&str> = ROLE_COLUMNS.iter().map(|(c, _)| *c).collect();
    let mut used = BTreeSet::new();
    let rows: Vec<Row> = lines
        .filter(|l| !l.starts_with("|---"))
        .map(|l| {
            let texts = split(l);
            assert_eq!(texts.len(), header.len(), "row {l}");
            let text = &texts[0];
            let matches: Vec<Permission> = ROWS
                .iter()
                .filter(|(prefix, _)| text.starts_with(prefix))
                .map(|(_, p)| *p)
                .collect();
            assert_eq!(matches.len(), 1, "row {text:?} must match one permission");
            assert!(used.insert(matches[0]), "row {text:?} repeats a permission");
            let cells: BTreeMap<String, Cell> = header[2..]
                .iter()
                .cloned()
                .zip(texts[2..].iter().map(|c| cell(c)))
                .collect();
            let self_row = cells.values().any(|c| *c == Cell::SelfOnly);
            if self_row {
                for (column, c) in &cells {
                    let want = match members.contains(&column.as_str()) {
                        true => Cell::SelfOnly,
                        false => Cell::Blank,
                    };
                    assert_eq!(*c, want, "the self row {text:?} at {column}");
                }
            }
            Row {
                permission: matches[0],
                step_up: step_up(&texts[1]),
                inactive: text.contains("only if DEC-437 item"),
                risk_reducing: RISK_REDUCING.iter().any(|p| text.starts_with(p)),
                reduction: text.starts_with("Pause")
                    || (text.starts_with("Kill switch") && text.ends_with(": engage")),
                self_row,
                own_row: texts[2..].iter().any(|c| c == "own")
                    || text.starts_with("Leave: deactivate one's own membership"),
                cells,
            }
        })
        .collect();
    for (prefix, permission) in ROWS {
        assert!(
            used.contains(&permission) || OWED.contains(&permission),
            "§4.2 has no row {prefix:?}"
        );
    }
    rows
}
