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
    trait_file: String,
    transport: Vec<Allowed>,
    denied: Denied,
}

#[derive(Deserialize)]
struct Allowed {
    #[serde(rename = "crate")]
    krate: String,
    #[serde(rename = "type")]
    ty: String,
    file: String,
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
    let lock: Lock = toml::from_str(&fs::read_to_string(root.join("Cargo.lock"))?)
        .context("parsing Cargo.lock")?;
    let locked: Vec<String> = lock.package.into_iter().map(|p| p.name).collect();
    let mut problems = transport_problems(&guard, &sources);
    problems.extend(crate_problems(&guard, &meta, &locked));
    report(problems, "mail-guard")
}

/// Every way `sources`, each `(crate, path, source)`, could reach a transport [`Guard`] does not
/// allow (DEC-2511 items 3 and 6). Every `MailTransport` token must stand in one of four places:
/// an `impl ... MailTransport for <Name>` in the allowed type's own file, naming it bare; the
/// trait's own definition in its file; a bound (`T: MailTransport`, `+ MailTransport`, `dyn` or
/// `impl MailTransport`); or a plain `use` path with no `as`. Anywhere else, a macro's arguments
/// included, it fails, so no macro can be handed the trait. In an allowed type's crate, its name
/// is declared once, by `struct` in its file, and is never given to anything else by `type`,
/// `enum`, `union` or `as`.
fn transport_problems(guard: &Guard, sources: &[(String, String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut declared: Vec<(&str, &str)> = Vec::new();
    for (krate, path, src) in sources {
        let toks = tokens(src);
        let in_macro = macro_arguments(&toks);
        for (i, (tok, line)) in toks.iter().enumerate() {
            let Token::Ident(word) = tok else {
                continue;
            };
            let at = |k: usize| toks.get(k).map(|(t, _)| t);
            let before = |back: usize| i.checked_sub(back).and_then(at);
            let next = at(i + 1);
            for allowed in guard.transport.iter().filter(|a| a.krate == *krate) {
                let declares = matches!(next, Some(Token::Ident(n)) if *n == allowed.ty);
                if declares && word == "struct" && *path == allowed.file {
                    declared.push((allowed.ty.as_str(), path.as_str()));
                } else if declares
                    && ["struct", "enum", "union", "type", "as"].contains(&word.as_str())
                {
                    problems.push(format!(
                        "{path}:{line}: `{word} {}` in `{krate}`; the allowed transport is declared \
                         once, by `struct` in {} ({GUARD})",
                        allowed.ty, allowed.file
                    ));
                }
            }
            if word != TRAIT {
                continue;
            }
            let placed = if in_macro.get(i).copied().unwrap_or(true) {
                Err("inside a macro's arguments".to_owned())
            } else if matches!(next, Some(Token::Ident(n)) if n == "for") {
                let ty = self_type(toks.get(i + 2..).unwrap_or_default());
                let allowed = guard
                    .transport
                    .iter()
                    .any(|a| a.krate == *krate && a.file == *path && Some(a.ty.as_str()) == ty);
                if allowed {
                    Ok(())
                } else {
                    Err(format!(
                        "implemented for `{}`; while the general footer is unresolved only the \
                         recorded fixture and the founder-only transport may be, each in its own \
                         file and by its bare name",
                        ty.unwrap_or("a type that is not a bare name")
                    ))
                }
            } else if matches!(before(1), Some(Token::Ident(w)) if w == "trait") {
                if *path == guard.trait_file {
                    Ok(())
                } else {
                    Err(format!("defined outside {}", guard.trait_file))
                }
            } else if matches!(next, Some(Token::Ident(n)) if n == "as") {
                Err("imported or re-exported under another name".to_owned())
            } else if in_use(&toks, i)
                || matches!(before(1), Some(Token::Punct('+')))
                || matches!(before(1), Some(Token::Ident(w)) if w == "dyn" || w == "impl")
                || (matches!(before(1), Some(Token::Punct(':')))
                    && !matches!(before(2), Some(Token::Punct(':'))))
            {
                Ok(())
            } else {
                Err(
                    "named where it is neither implemented, defined, a bound, nor imported"
                        .to_owned(),
                )
            };
            if let Err(why) = placed {
                problems.push(format!(
                    "{path}:{line}: `{TRAIT}` {why} in `{krate}` ({GUARD})"
                ));
            }
        }
    }
    for allowed in &guard.transport {
        let count = declared.iter().filter(|(ty, _)| *ty == allowed.ty).count();
        if count > 1 {
            problems.push(format!(
                "{}: `struct {}` is declared {count} times; once is allowed ({GUARD})",
                allowed.file, allowed.ty
            ));
        }
    }
    problems
}

/// Every string literal in `sources` outside the exempt crates that names a mail endpoint: an SMTP
/// URL, `sendmail`, a mail provider's API host, or a host on a mail submission port.
fn literal_problems(guard: &Guard, sources: &[(String, String, String)]) -> Vec<String> {
    let _ = (guard, sources);
    Vec::new()
}

/// Whether each token lies inside a macro invocation's delimited arguments (`name!(...)`,
/// `name![...]`, `name!{...}`), so no macro can be handed the trait to build an impl from. A token
/// the scan cannot place reads as inside, failing closed.
fn macro_arguments(toks: &[(Token, usize)]) -> Vec<bool> {
    let mut inside = vec![false; toks.len()];
    for (i, window) in toks.windows(3).enumerate() {
        let opens = match (&window[0].0, &window[1].0, &window[2].0) {
            (Token::Ident(_), Token::Punct('!'), Token::Punct(c)) => match c {
                '(' => Some((')', '(')),
                '[' => Some((']', '[')),
                '{' => Some(('}', '{')),
                _ => None,
            },
            _ => None,
        };
        let Some((close, open)) = opens else {
            continue;
        };
        let mut depth = 0usize;
        let mut end = toks.len();
        for (k, (t, _)) in toks.iter().enumerate().skip(i + 2) {
            match t {
                Token::Punct(c) if *c == open => depth += 1,
                Token::Punct(c) if *c == close => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = k;
                        break;
                    }
                }
                _ => {}
            }
        }
        for flag in inside.iter_mut().take(end).skip(i + 3) {
            *flag = true;
        }
    }
    inside
}

/// Whether the token at `i` is a segment of a `use` path: walking back over path tokens (names,
/// `::`, `{`, `,`, `*`) reaches `use` before anything else.
fn in_use(toks: &[(Token, usize)], i: usize) -> bool {
    for (tok, _) in toks.iter().take(i).rev() {
        match tok {
            Token::Ident(word) if word == "use" => return true,
            Token::Ident(_) | Token::Punct(':' | '{' | ',' | '*') => {}
            _ => return false,
        }
    }
    false
}

/// The name of an impl's self type, read from the tokens after `for` up to `{` or `where`, when
/// that type is one bare name; `None` for any other type, so a path, a reference, a tuple, a
/// generic or a `dyn` never passes as an allowed one.
fn self_type(toks: &[(Token, usize)]) -> Option<&str> {
    match toks {
        [(Token::Ident(name), _), (end, _), ..]
            if name != "dyn"
                && (matches!(end, Token::Punct('{'))
                    || matches!(end, Token::Ident(w) if w == "where")) =>
        {
            Some(name.as_str())
        }
        _ => None,
    }
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
        let file = match krate {
            "mandate-dispatcher" => "crates/mandate-dispatcher/src/email.rs".to_owned(),
            "mandate-mail-smtp" => "crates/mandate-mail-smtp/src/lib.rs".to_owned(),
            other => format!("crates/{other}/src/lib.rs"),
        };
        (krate.to_owned(), file, src.to_owned())
    }

    fn elsewhere(krate: &str, src: &str) -> (String, String, String) {
        (
            krate.to_owned(),
            format!("crates/{krate}/src/other.rs"),
            src.to_owned(),
        )
    }

    fn fails(sources: &[(String, String, String)]) -> Vec<String> {
        let problems = transport_problems(&guard(), sources);
        assert!(
            !problems.is_empty(),
            "{:?} passes",
            sources.iter().map(|s| &s.2).collect::<Vec<_>>()
        );
        problems
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
    fn the_allowed_implementors_pass_in_their_own_files() {
        let sources = [
            source(
                "mandate-dispatcher",
                "pub trait MailTransport { }\npub struct RecordedTransport;\n\
                 impl MailTransport for RecordedTransport { }\n\
                 impl<T: MailTransport> Provider for EmailProvider<T> where T: Send + MailTransport { }\n\
                 fn f(t: &mut dyn MailTransport, u: impl MailTransport) { }",
            ),
            source(
                "mandate-mail-smtp",
                "use mandate_dispatcher::{FounderMail, MailTransport};\npub struct FounderSmtpTransport;\n\
                 impl MailTransport for FounderSmtpTransport where Self: Sized { }",
            ),
            elsewhere(
                "mandate-dispatcher",
                "pub use email::{EmailProvider, MailTransport, RecordedTransport};",
            ),
            elsewhere(
                "mandate-dispatcher",
                "/// impl MailTransport for Other\nconst X: &str = \"impl MailTransport for Other\";",
            ),
            elsewhere("mandate-gateway", "pub struct RecordedTransport;"),
        ];
        assert_eq!(transport_problems(&guard(), &sources), Vec::<String>::new());
    }

    #[test]
    fn any_other_implementor_fails() {
        for (krate, src) in [
            (
                "mandate-dispatcher",
                "impl MailTransport for SmtpTransport { }",
            ),
            (
                "mandate-api",
                "impl MailTransport for RecordedTransport { }",
            ),
            (
                "mandate-dispatcher",
                "impl<T: Send> MailTransport for T { }",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for Box<dyn Send> { }",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for &mut RecordedTransport { }",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for (RecordedTransport, E) { }",
            ),
            ("mandate-dispatcher", "impl MailTransport for dyn Send { }"),
            (
                "mandate-dispatcher",
                "impl MailTransport for evil::RecordedTransport { }",
            ),
            (
                "mandate-dispatcher",
                "impl MailTransport for crate::email::RecordedTransport { }",
            ),
        ] {
            fails(&[source(krate, src)]);
        }
        fails(&[elsewhere(
            "mandate-dispatcher",
            "impl MailTransport for RecordedTransport { }",
        )]);
    }

    #[test]
    fn a_macro_handed_the_trait_fails() {
        for src in [
            "macro_rules! m { ($t:path, $s:ty) => { impl $t for $s {} } }\nm!(MailTransport, Evil);",
            "macro_rules! m { ($t:ident) => { use crate::email::$t as T; } }\nm!(MailTransport);",
            "m!(x: MailTransport, Evil);",
            "m![crate::email::MailTransport];",
            "macro_rules! m { ($s:ty) => { impl MailTransport for $s {} } }",
        ] {
            fails(&[elsewhere("mandate-dispatcher", src)]);
        }
    }

    #[test]
    fn a_renamed_trait_or_a_borrowed_name_fails() {
        for src in [
            "use mandate_dispatcher::MailTransport as Transport;",
            "pub use email::{MailTransport as Sender};",
            "type RecordedTransport = Evil;",
            "use evil::Evil as RecordedTransport;",
            "pub enum RecordedTransport { }",
            "pub trait MailTransport { }",
            "fn f() -> MailTransport::Error { }",
        ] {
            fails(&[elsewhere("mandate-dispatcher", src)]);
        }
        fails(&[source(
            "mandate-dispatcher",
            "pub struct RecordedTransport;\nmod evil { pub struct RecordedTransport; }",
        )]);
        fails(&[elsewhere(
            "mandate-mail-smtp",
            "pub type FounderSmtpTransport = lettre::SmtpTransport;",
        )]);
    }

    #[test]
    fn an_allowed_type_declared_other_than_once_at_the_top_of_its_file_fails() {
        let trait_def = "pub trait MailTransport { }\n";
        for (what, body) in [
            (
                "a foreign type imported under the allowed name",
                "pub struct Recorded;\nuse mandate_gateway::RecordedTransport;\nimpl MailTransport for RecordedTransport { }",
            ),
            (
                "no declaration at all",
                "impl MailTransport for RecordedTransport { }",
            ),
            (
                "a declaration inside a nested module",
                "mod shadow { pub struct RecordedTransport; }\nuse shadow::RecordedTransport;\nimpl MailTransport for RecordedTransport { }",
            ),
            (
                "a declaration inside a function",
                "fn f() { struct RecordedTransport; }\nimpl MailTransport for RecordedTransport { }",
            ),
            (
                "an import naming the allowed type beside its declaration",
                "pub struct RecordedTransport;\nuse self::RecordedTransport as _;\nuse crate::RecordedTransport;\nimpl MailTransport for RecordedTransport { }",
            ),
        ] {
            let problems = transport_problems(
                &guard(),
                &[source("mandate-dispatcher", &format!("{trait_def}{body}"))],
            );
            assert!(!problems.is_empty(), "{what} passes");
        }
    }

    #[test]
    fn an_absent_crate_owes_no_declaration() {
        let only_dispatcher = [source(
            "mandate-dispatcher",
            "pub trait MailTransport { }\npub struct RecordedTransport;\nimpl MailTransport for RecordedTransport { }",
        )];
        assert_eq!(
            transport_problems(&guard(), &only_dispatcher),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_mail_endpoint_literal_outside_the_founder_transport_fails() {
        for literal in [
            "smtp://mail.example:587",
            "SMTPS://mail.example",
            "sendmail",
            "/usr/sbin/sendmail",
            "https://api.sendgrid.com/v3/mail/send",
            "api.mailgun.net",
            "mail.example:25",
            "127.0.0.1:465",
            "mail.example:2525",
        ] {
            let src = format!("const X: &str = {literal:?};");
            assert!(
                !literal_problems(&guard(), &[elsewhere("mandate-dispatcher", &src)]).is_empty(),
                "{literal} passes"
            );
            assert_eq!(
                literal_problems(&guard(), &[source("mandate-mail-smtp", &src)]),
                Vec::<String>::new(),
                "{literal} in the founder transport"
            );
        }
        for harmless in [
            "12:25:00",
            "2026-10-17T12:25:00Z",
            "port 25",
            "mail.example:443",
            "sentmail",
        ] {
            let src = format!("const X: &str = {harmless:?};");
            assert_eq!(
                literal_problems(&guard(), &[elsewhere("mandate-dispatcher", &src)]),
                Vec::<String>::new(),
                "{harmless}"
            );
        }
        let in_xtask = elsewhere("xtask", "const X: &str = \"smtp://mail.example:587\";");
        assert_eq!(
            literal_problems(&guard(), &[in_xtask]),
            Vec::<String>::new()
        );
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
