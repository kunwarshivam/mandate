//! The deferring guard (DEC-2378 item 11 condition 3, rung 2): only the cell's runtime's advisory
//! dry run may defer a gate check, so the executor's binding gate always evaluates every check.
//! `clippy.toml` disallows `mandate_risk::evaluate_deferring` and `mandate_ports::evaluate_deferring`,
//! but an `#[allow(clippy::disallowed_methods)]` silences that lint for every path at once, and
//! the executor's host already carries one for its clock. This check cannot be silenced: no Rust
//! source outside [`ALLOWED`] may name `evaluate_deferring` at all, in its code, its tests, or its
//! docs, so a binding-side crate can neither call it nor be shown how. It runs in the lint job.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::{files_by_extension, metadata_in, report, workspace_packages};

/// The name only [`ALLOWED`] may write.
const NAME: &str = "evaluate_deferring";

/// The crates that may name it: `mandate-risk`, which defines it and pins it against the binding
/// gate in its tests; `mandate-ports`, whose wrapper is the one entry the advisory dry run calls;
/// `mandate-daemon`, the cell's runtime, whose dry run is that one caller; and `xtask`, this guard.
const ALLOWED: [&str; 4] = ["mandate-risk", "mandate-ports", "mandate-daemon", "xtask"];

/// The guard's own crate, whose sources spell out what they refuse.
const GUARD: &str = "xtask";

/// Runs the guard over the workspace at `root`.
pub(crate) fn guard_in(root: &Path) -> Result<()> {
    eprintln!(
        "    deferring-guard: checking that only the advisory dry run names `{NAME}` (DEC-2378 item 11)"
    );
    let meta = metadata_in(root)?;
    let base = fs::canonicalize(root).with_context(|| format!("resolving {}", root.display()))?;
    let mut sources = Vec::new();
    for pkg in workspace_packages(&meta) {
        let Some(dir) = pkg.manifest_path.parent() else {
            continue;
        };
        for file in files_by_extension(dir, &["rs"])? {
            let src =
                fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
            let shown = file
                .strip_prefix(&base)
                .unwrap_or(&file)
                .display()
                .to_string();
            sources.push((pkg.name.clone(), shown, src));
        }
    }
    report(problems(&sources), "deferring-guard")
}

/// Every source of `sources`, each `(crate, path, source)`, outside [`ALLOWED`] that names
/// [`NAME`], with the first line that does; and every source, inside [`ALLOWED`] or not, that
/// renames it with `as`, which would let a crate outside [`ALLOWED`] call it under another name;
/// this guard's own crate, [`GUARD`], spells both out and is exempt.
fn problems(sources: &[(String, String, String)]) -> Vec<String> {
    let renamed = format!("{NAME} as");
    let named = sources
        .iter()
        .filter(|(krate, _, _)| !ALLOWED.contains(&krate.as_str()))
        .filter_map(|(krate, path, src)| {
            src.lines()
                .position(|line| line.contains(NAME))
                .map(|index| {
                    format!(
                        "{path}:{}: `{krate}` names `{NAME}`; only {} may, so the binding gate always evaluates every check",
                        index + 1,
                        ALLOWED.join(", ")
                    )
                })
        });
    let aliased = sources.iter().filter(|(krate, _, _)| krate != GUARD).filter_map(|(krate, path, src)| {
        src.lines()
            .position(|line| line.contains(&renamed))
            .map(|index| {
                format!(
                    "{path}:{}: `{krate}` renames `{NAME}`, which would let another crate call it unnamed",
                    index + 1
                )
            })
    });
    named.chain(aliased).collect()
}

#[cfg(test)]
mod tests {
    use super::{ALLOWED, NAME, problems};

    fn source(krate: &str, src: &str) -> (String, String, String) {
        (
            krate.to_owned(),
            format!("crates/{krate}/src/lib.rs"),
            src.to_owned(),
        )
    }

    #[test]
    fn the_binding_crates_may_not_name_it_even_under_an_allow() {
        for krate in [
            "mandate-executor",
            "mandate-executord",
            "mandate-gate-inputs",
            "mandate-shell",
        ] {
            let found = problems(&[source(
                krate,
                "#[allow(clippy::disallowed_methods, reason = \"x\")]\nfn f() { mandate_ports::evaluate_deferring(); }\n",
            )]);
            assert_eq!(found.len(), 1, "{krate}: {found:?}");
            assert!(found[0].contains(":2:"), "it names the line: {found:?}");
        }
    }

    #[test]
    fn a_new_crate_may_not_name_it_either() {
        assert_eq!(
            problems(&[source(
                "mandate-anything",
                "//! See `evaluate_deferring`.\n"
            )])
            .len(),
            1
        );
    }

    #[test]
    fn no_crate_may_rename_it_not_even_an_allowed_one() {
        let found = problems(&[source(
            "mandate-ports",
            "pub use gate::evaluate_deferring as advise;\n",
        )]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("renames"), "{found:?}");
    }

    #[test]
    fn the_allowed_crates_and_other_names_pass() {
        let mut sources: Vec<_> = ALLOWED
            .iter()
            .map(|krate| source(krate, &format!("fn g() {{ {NAME}(); }}\n")))
            .collect();
        sources.push(source(
            "mandate-executor",
            "fn h() { mandate_risk::evaluate(); }\n",
        ));
        assert!(problems(&sources).is_empty(), "{:?}", problems(&sources));
    }
}
