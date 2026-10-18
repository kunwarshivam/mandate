//! The mail guard (notifications spec §4.4, rung 2; DEC-700 item 4, DEC-2511): while the general
//! email footer is unresolved, no mail may reach anyone but the founder. The adapter's types hold
//! that at rung 1. This check holds it at rung 2: it refuses any `MailTransport` implementor but
//! the two [`GUARD`] lists, each in its own crate, and any mail-sending crate in the build except
//! as a direct dependency of the founder-only transport's crate. It runs in the lint job while the
//! spec still holds the footer's placeholder.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{
    Metadata, Token, files_by_extension, metadata_in, repo_root, report, tokens, workspace_packages,
};

/// The guard's configuration, beside the other `xtask` policy files.
pub(crate) const GUARD: &str = "xtask/mail-guard.toml";

/// The trait whose implementors are guarded.
const TRAIT: &str = "MailTransport";

#[derive(Deserialize)]
pub(crate) struct Guard {
    spec: String,
    placeholder: String,
    founder_transport_crate: String,
    transport: Vec<Allowed>,
    denied: Denied,
}

#[derive(Deserialize)]
struct Allowed {
    #[serde(rename = "crate")]
    krate: String,
    #[serde(rename = "type")]
    ty: String,
}

#[derive(Deserialize)]
struct Denied {
    crates: Vec<String>,
}

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Locked>,
}

#[derive(Deserialize)]
struct Locked {
    name: String,
}

/// Runs the guard over the workspace at `root`, unless the spec no longer holds the footer's
/// placeholder. The configuration and the spec are always this repository's, as
/// `decision_numbers`'s script is, so a tree without them is still guarded.
pub(crate) fn guard_in(root: &Path) -> Result<()> {
    eprintln!("    mail-guard: checking that no mail can reach anyone but the founder (DEC-2511)");
    let home = repo_root()?;
    let config =
        fs::read_to_string(home.join(GUARD)).with_context(|| format!("reading {GUARD}"))?;
    let guard: Guard = toml::from_str(&config).with_context(|| format!("parsing {GUARD}"))?;
    let spec = fs::read_to_string(home.join(&guard.spec))
        .with_context(|| format!("reading {}", guard.spec))?;
    if !spec.contains(&guard.placeholder) {
        return Ok(());
    }
    let meta = metadata_in(root)?;
    let mut sources = Vec::new();
    for pkg in workspace_packages(&meta) {
        let Some(dir) = pkg.manifest_path.parent() else {
            continue;
        };
        for file in files_by_extension(dir, &["rs"])? {
            let src =
                fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
            let shown = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .display()
                .to_string();
            sources.push((pkg.name.clone(), shown, src));
        }
    }
    let lock: Lock = toml::from_str(&fs::read_to_string(root.join("Cargo.lock"))?)
        .context("parsing Cargo.lock")?;
    let locked: Vec<String> = lock.package.into_iter().map(|p| p.name).collect();
    let mut problems = transport_problems(&guard, &sources);
    problems.extend(crate_problems(&guard, &meta, &locked));
    report(problems, "mail-guard")
}

/// Every way `sources`, each `(crate, path, source)`, could reach a transport [`Guard`] does not
/// allow: an `impl ... MailTransport for <Type>` whose self type is not exactly an allowed type's
/// path in that type's crate (a reference, a tuple, a generic, or a type parameter fails), the
/// trait imported or re-exported under another name, and an allowed type's name given to anything
/// else by `type` or `as`.
fn transport_problems(guard: &Guard, sources: &[(String, String, String)]) -> Vec<String> {
    let names: BTreeSet<&str> = guard.transport.iter().map(|a| a.ty.as_str()).collect();
    let mut problems = Vec::new();
    for (krate, path, src) in sources {
        let toks = tokens(src);
        for (i, (tok, line)) in toks.iter().enumerate() {
            let Token::Ident(word) = tok else {
                continue;
            };
            let next = match toks.get(i + 1) {
                Some((Token::Ident(next), _)) => Some(next.as_str()),
                _ => None,
            };
            if word == TRAIT && next == Some("as") {
                problems.push(format!(
                    "{path}:{line}: `{TRAIT}` is imported or re-exported under another name in \
                     `{krate}`; the guard reads the trait by its own name ({GUARD})"
                ));
            }
            if let Some(name) =
                next.filter(|n| (word == "type" || word == "as") && names.contains(n))
            {
                problems.push(format!(
                    "{path}:{line}: `{name}` is given to another type by `{word}` in `{krate}`; an \
                     allowed transport's name may name only that type ({GUARD})"
                ));
            }
            if word != TRAIT || next != Some("for") {
                continue;
            }
            let ty = self_type(toks.get(i + 2..).unwrap_or_default());
            let allowed = guard
                .transport
                .iter()
                .any(|a| a.krate == *krate && Some(a.ty.as_str()) == ty);
            if !allowed {
                problems.push(format!(
                    "{path}:{line}: `{TRAIT}` is implemented for `{}` in `{krate}`; while the \
                     general footer is unresolved only the recorded fixture and the founder-only \
                     transport may be ({GUARD})",
                    ty.unwrap_or("a type that is not a plain path")
                ));
            }
        }
    }
    problems
}

/// The last segment of an impl's self type, read from the tokens after `for` up to `{` or
/// `where`, when that type is a plain path (`Name` or `a::b::Name`); `None` for any other type,
/// so a reference, a tuple, a generic, a `dyn`, or a leading `::` never passes as an allowed one.
fn self_type(toks: &[(Token, usize)]) -> Option<&str> {
    let (mut last, mut expect) = (None, Expect::Segment);
    for (tok, _) in toks {
        match (tok, expect) {
            (Token::Punct('{'), Expect::Separator) => return last,
            (Token::Ident(word), Expect::Separator) if word == "where" => return last,
            (Token::Ident(word), Expect::Segment) if word != "dyn" => {
                last = Some(word.as_str());
                expect = Expect::Separator;
            }
            (Token::Punct(':'), Expect::Separator) => expect = Expect::SecondColon,
            (Token::Punct(':'), Expect::SecondColon) => expect = Expect::Segment,
            _ => return None,
        }
    }
    None
}

/// What [`self_type`] may read next.
#[derive(Clone, Copy)]
enum Expect {
    Segment,
    Separator,
    SecondColon,
}

/// Every denied crate a workspace member other than the founder-only transport's crate depends on
/// directly, and every denied crate in `locked` that the founder-only transport's crate does not
/// depend on directly, so a mail crate never enters the build through anything else.
fn crate_problems(guard: &Guard, meta: &Metadata, locked: &[String]) -> Vec<String> {
    let denied: BTreeSet<&str> = guard.denied.crates.iter().map(String::as_str).collect();
    let founder = guard.founder_transport_crate.as_str();
    let mut problems = Vec::new();
    let mut founders_own = BTreeSet::new();
    for pkg in workspace_packages(meta) {
        for dep in &pkg.dependencies {
            if !denied.contains(dep.name.as_str()) {
                continue;
            }
            if pkg.name == founder {
                founders_own.insert(dep.name.as_str());
            } else {
                problems.push(format!(
                    "`{}` depends on the mail-sending crate `{}`; only `{founder}` may ({GUARD})",
                    pkg.name, dep.name
                ));
            }
        }
    }
    let in_build: BTreeSet<&str> = locked
        .iter()
        .map(String::as_str)
        .filter(|name| denied.contains(name))
        .collect();
    for name in in_build.difference(&founders_own) {
        problems.push(format!(
            "the mail-sending crate `{name}` is in Cargo.lock but is no direct dependency of \
             `{founder}`; nothing else may bring it into the build ({GUARD})"
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{Dependency, Package};

    fn guard() -> Guard {
        toml::from_str(&fs::read_to_string(crate::repo_root().unwrap().join(GUARD)).unwrap())
            .unwrap()
    }

    fn source(krate: &str, src: &str) -> (String, String, String) {
        (
            krate.to_owned(),
            format!("crates/{krate}/src/lib.rs"),
            src.to_owned(),
        )
    }

    fn package(name: &str, deps: &[&str]) -> Package {
        Package {
            id: format!("{name} 0.0.0"),
            name: name.to_owned(),
            manifest_path: PathBuf::from(format!("crates/{name}/Cargo.toml")),
            dependencies: deps
                .iter()
                .map(|d| Dependency {
                    name: (*d).to_owned(),
                    kind: None,
                    path: None,
                    features: Vec::new(),
                    uses_default_features: true,
                })
                .collect(),
            targets: Vec::new(),
            features: Default::default(),
        }
    }

    fn meta(packages: Vec<Package>) -> Metadata {
        let members = packages.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        Metadata {
            packages,
            workspace_default_members: members.clone(),
            workspace_members: members,
        }
    }

    #[test]
    fn the_repository_passes_the_guard() {
        guard_in(&crate::repo_root().unwrap()).unwrap();
    }

    #[test]
    fn the_allowed_implementors_pass_in_their_own_crates() {
        let sources = [
            source(
                "mandate-dispatcher",
                "impl MailTransport for RecordedTransport { }",
            ),
            source(
                "mandate-mail-smtp",
                "impl mandate_dispatcher::MailTransport for FounderSmtpTransport { }",
            ),
            source(
                "mandate-dispatcher",
                "/// impl MailTransport for Other\nconst X: &str = \"impl MailTransport for Other\";",
            ),
        ];
        assert_eq!(transport_problems(&guard(), &sources), Vec::<String>::new());
    }

    #[test]
    fn any_other_implementor_fails() {
        let cases = [
            (
                "mandate-dispatcher",
                "impl MailTransport for SmtpTransport { }",
                "`SmtpTransport`",
            ),
            (
                "mandate-api",
                "impl MailTransport for RecordedTransport { }",
                "`RecordedTransport`",
            ),
            (
                "mandate-dispatcher",
                "impl<T: Send> MailTransport for T { }",
                "`T`",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for Box<dyn Send> { }",
                "not a plain path",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for &mut RecordedTransport { }",
                "not a plain path",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for (RecordedTransport, E) { }",
                "not a plain path",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for dyn Send { }",
                "not a plain path",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for ::evil::RecordedTransport { }",
                "not a plain path",
            ),
        ];
        for (krate, src, named) in cases {
            let problems = transport_problems(&guard(), &[source(krate, src)]);
            assert_eq!(problems.len(), 1, "{src}: {problems:?}");
            assert!(problems[0].contains(named), "{src}: {problems:?}");
        }
    }

    #[test]
    fn an_allowed_path_or_where_clause_still_passes() {
        let sources = [
            source(
                "mandate-dispatcher",
                "impl MailTransport for crate::email::RecordedTransport { }",
            ),
            source(
                "mandate-dispatcher",
                "impl MailTransport for RecordedTransport where Self: Sized { }",
            ),
        ];
        assert_eq!(transport_problems(&guard(), &sources), Vec::<String>::new());
    }

    #[test]
    fn a_renamed_trait_or_a_borrowed_name_fails() {
        let cases = [
            "use mandate_dispatcher::MailTransport as Transport;",
            "pub use email::{MailTransport as Sender};",
            "type RecordedTransport = Evil;",
            "use evil::Evil as RecordedTransport;",
            "pub type FounderSmtpTransport = lettre::SmtpTransport;",
        ];
        for src in cases {
            let problems = transport_problems(&guard(), &[source("mandate-dispatcher", src)]);
            assert_eq!(problems.len(), 1, "{src}: {problems:?}");
        }
    }

    #[test]
    fn a_mail_crate_anywhere_but_the_founder_transport_fails() {
        let other = meta(vec![package(
            "mandate-dispatcher",
            &["lettre", "thiserror"],
        )]);
        let problems = crate_problems(&guard(), &other, &["lettre".to_owned()]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems.iter().all(|p| p.contains("`lettre`")),
            "{problems:?}"
        );

        let transitive = meta(vec![package("mandate-dispatcher", &["some-mailer"])]);
        let problems = crate_problems(&guard(), &transitive, &["mail-send".to_owned()]);
        assert_eq!(
            problems.len(),
            1,
            "a denied crate only in the lock: {problems:?}"
        );
    }

    #[test]
    fn the_founder_transport_may_depend_on_a_mail_crate() {
        let founder = meta(vec![package("mandate-mail-smtp", &["lettre"])]);
        assert_eq!(
            crate_problems(&guard(), &founder, &["lettre".to_owned()]),
            Vec::<String>::new()
        );
    }
}
