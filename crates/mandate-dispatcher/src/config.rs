//! The dispatcher's configuration file (DEC-2500 item 7, DEC-2516 item 2): one JSON object, read
//! once at start, holding no secret and no address. Unknown members are refused, and so is
//! anything the cell's dispatcher cannot honour: web push switched on (DEC-2504's interim keeps it
//! off), a channel with no adapter, or more than one email recipient (the founder-only transport
//! mails the founder alone). A refusal names its member, never its value.

use std::collections::BTreeSet;

use mandate_canon::{Value, parse};
use mandate_journal::{Environment, StreamId};
use mandate_notify::{Origin, PushChannel, Recipient};

use crate::process::Settings;

/// Every member the file may hold.
const MEMBERS: [&str; 9] = [
    "workspace",
    "subjects",
    "audience",
    "app_origin",
    "environment",
    "build",
    "web_push",
    "push_allowlist",
    "vapid_subject",
];

/// Why a configuration file was refused: the member at fault, never its value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("configuration refused at `{member}`")]
pub struct ConfigError {
    pub member: &'static str,
}

/// A configuration the process can run: its settings and the one recipient the email channel
/// mails.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub settings: Settings,
    pub founder: Recipient,
}

fn refused(member: &'static str) -> ConfigError {
    ConfigError { member }
}

/// Reads the file's bytes.
///
/// # Errors
/// [`ConfigError`] naming the first member that is missing, mistyped, unknown, or not one the
/// cell's dispatcher can honour.
pub fn read(bytes: &[u8]) -> Result<Loaded, ConfigError> {
    let document = parse(bytes).map_err(|_| refused("document"))?;
    let Value::Object(members) = &document else {
        return Err(refused("document"));
    };
    if members.keys().any(|key| !MEMBERS.contains(&key.as_str())) {
        return Err(refused("unknown"));
    }
    let text = |member: &'static str| {
        document
            .get(member)
            .and_then(Value::as_str)
            .ok_or_else(|| refused(member))
    };
    let list = |member: &'static str| match document.get(member) {
        Some(Value::Array(items)) => Ok(items.as_slice()),
        _ => Err(refused(member)),
    };
    let workspace = text("workspace")?;
    if StreamId::parse(&format!("ntf:{workspace}")).is_none() {
        return Err(refused("workspace"));
    }
    let floor = list("subjects")?
        .iter()
        .map(|item| {
            item.as_str()
                .and_then(StreamId::parse)
                .ok_or_else(|| refused("subjects"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut audience = Vec::new();
    for entry in list("audience")? {
        let member = |name: &str| entry.get(name).and_then(Value::as_str);
        let named = member("recipient").ok_or_else(|| refused("audience"))?;
        let recipient = Recipient::parse(named).map_err(|_| refused("audience"))?;
        let channel = match member("channel") {
            Some("email") => PushChannel::Email,
            _ => return Err(refused("audience")),
        };
        audience.push((named.to_owned(), recipient, channel));
    }
    let mailed: BTreeSet<&str> = audience
        .iter()
        .map(|(named, _, _)| named.as_str())
        .collect();
    let founder = match (mailed.len(), audience.first()) {
        (1, Some((_, recipient, _))) => recipient.clone(),
        _ => return Err(refused("audience")),
    };
    let origin = Origin::parse(text("app_origin")?).map_err(|_| refused("app_origin"))?;
    let environment = match text("environment")? {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        _ => return Err(refused("environment")),
    };
    let build = text("build")?;
    let digest = build.strip_prefix("sha256:").unwrap_or_default();
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(refused("build"));
    }
    if document.get("web_push") != Some(&Value::Bool(false)) {
        return Err(refused("web_push"));
    }
    if list("push_allowlist")?
        .iter()
        .any(|item| item.as_str().is_none())
    {
        return Err(refused("push_allowlist"));
    }
    match document.get("vapid_subject") {
        Some(Value::Null | Value::Str(_)) => {}
        _ => return Err(refused("vapid_subject")),
    }
    Ok(Loaded {
        settings: Settings {
            workspace: workspace.to_owned(),
            floor,
            audience: audience
                .into_iter()
                .map(|(named, _, channel)| (named, channel))
                .collect(),
            origin,
            environment,
            build: build.to_owned(),
        },
        founder,
    })
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, read};
    use mandate_notify::PushChannel;

    /// The good file's members, in order, each as its JSON text.
    const GOOD: [(&str, &str); 9] = [
        ("workspace", r#""ws_1""#),
        ("subjects", r#"["acct:ws_1:A1"]"#),
        (
            "audience",
            r#"[{"recipient":"p_founder","channel":"email"}]"#,
        ),
        ("app_origin", r#""https://app.example.invalid""#),
        ("environment", r#""paper""#),
        (
            "build",
            r#""sha256:3333333333333333333333333333333333333333333333333333333333333333""#,
        ),
        ("web_push", "false"),
        ("push_allowlist", "[]"),
        ("vapid_subject", "null"),
    ];

    /// The good file with `member` set to `value` (added when the good file has no such member).
    fn with(member: &str, value: &str) -> String {
        let mut members: Vec<(String, String)> = GOOD
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        match members.iter_mut().find(|(k, _)| k == member) {
            Some(entry) => value.clone_into(&mut entry.1),
            None => members.push((member.to_owned(), value.to_owned())),
        }
        let rendered: Vec<String> = members
            .iter()
            .map(|(k, v)| format!("\"{k}\": {v}"))
            .collect();
        format!("{{\n  {}\n}}", rendered.join(",\n  "))
    }

    /// The file a cell mounts reads as its settings: the workspace, the floor, the founder on the
    /// email channel, the origin, the environment and the build.
    #[test]
    fn a_closed_file_reads_as_its_settings() -> Result<(), ConfigError> {
        let loaded = read(with("workspace", r#""ws_1""#).as_bytes())?;
        assert_eq!(loaded.settings.workspace, "ws_1");
        assert_eq!(
            loaded
                .settings
                .floor
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
            ["acct:ws_1:A1"]
        );
        assert_eq!(
            loaded.settings.audience,
            [("p_founder".to_owned(), PushChannel::Email)]
        );
        assert_eq!(
            loaded.founder,
            mandate_notify::Recipient::parse("p_founder")
                .map_err(|_| ConfigError { member: "test" })?
        );
        Ok(())
    }

    /// Each refusal names its member: an unknown one, web push on, a channel with no adapter, a
    /// second email recipient, no recipient, a malformed build, a stream that is not one.
    #[test]
    fn anything_the_cell_cannot_honour_is_refused_at_its_member() {
        for (member, value, at) in [
            ("extra", "1", "unknown"),
            ("web_push", "true", "web_push"),
            ("web_push", "null", "web_push"),
            (
                "audience",
                r#"[{"recipient":"p_founder","channel":"web_push"}]"#,
                "audience",
            ),
            (
                "audience",
                r#"[{"recipient":"p_a","channel":"email"},{"recipient":"p_b","channel":"email"}]"#,
                "audience",
            ),
            ("audience", "[]", "audience"),
            ("build", r#""sha256:33""#, "build"),
            ("subjects", r#"["not a stream"]"#, "subjects"),
            ("environment", r#""staging""#, "environment"),
            ("vapid_subject", "7", "vapid_subject"),
            ("push_allowlist", "[7]", "push_allowlist"),
            ("app_origin", r#""not an origin""#, "app_origin"),
            ("workspace", "7", "workspace"),
        ] {
            let text = with(member, value);
            assert_eq!(
                read(text.as_bytes()).map(|_| ()),
                Err(ConfigError { member: at }),
                "{member} = {value}"
            );
        }
        assert_eq!(
            read(b"[]").map(|_| ()),
            Err(ConfigError { member: "document" })
        );
    }
}
