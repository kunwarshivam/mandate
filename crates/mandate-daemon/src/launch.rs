//! Where a runtime process learns its deployment (DEC-1015): from `run`'s flags, or from the
//! variables the deployment manager sets on the pod it starts (E21-33), ids only and never a
//! secret.
//!
//! The manager sets `MANDATE_PROCESS` (`runtime` here), `MANDATE_WORKSPACE`, `MANDATE_SUBJECT` (the
//! agent) and `MANDATE_GENERATION`, which only the manager reads: the agent stream's writer epoch
//! is what fences an earlier process (DEC-1012). A flag never overrides a variable the manager set,
//! and one that disagrees with it is refused rather than guessed between (`AGENTS.md` rule 3). The
//! connection and the owner may be left unnamed, to be read from the agent's deployment.
//! [`gateway`] reads the model-gateway service's address a researching runtime calls (DEC-2397).

use crate::remote::GatewayAddress;

/// The deployment a launch names; the connection and the owner are `None` when it leaves them to
/// be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub workspace: String,
    pub agent: String,
    pub connection: Option<String>,
    pub owner: Option<String>,
}

/// The deployment `flag` (the value after a `run` flag) and `var` (an environment variable, `Some`
/// for any value it holds, unreadable ones included) name, or the refusal's code:
/// `process_not_runtime` for a pod started for another process, `deployment_missing` for no
/// workspace or agent, and `deployment_conflict` for a flag that disagrees with the manager's
/// variable, or a manager variable set without `MANDATE_PROCESS`. A value that is empty or has
/// whitespace around it names nothing.
///
/// # Errors
/// The refusal codes above.
pub fn named(
    flag: &dyn Fn(&str) -> Option<String>,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<Named, &'static str> {
    let given = |value: Option<String>| value.filter(|v| !v.is_empty() && v.trim() == v);
    let (workspace, agent) = match given(var("MANDATE_PROCESS")) {
        None if var("MANDATE_PROCESS").is_some() => return Err("process_not_runtime"),
        None if var("MANDATE_WORKSPACE").is_some() || var("MANDATE_SUBJECT").is_some() => {
            return Err("deployment_conflict");
        }
        None => (given(flag("--workspace")), given(flag("--agent"))),
        Some(process) if process != "runtime" => return Err("process_not_runtime"),
        Some(_) => (
            agreed(var("MANDATE_WORKSPACE"), flag("--workspace"))?,
            agreed(var("MANDATE_SUBJECT"), flag("--agent"))?,
        ),
    };
    let (Some(workspace), Some(agent)) = (given(workspace), given(agent)) else {
        return Err("deployment_missing");
    };
    Ok(Named {
        workspace,
        agent,
        connection: given(flag("--connection")),
        owner: given(flag("--owner")),
    })
}

/// The manager's `set` value, unless a flag names a different one.
fn agreed(set: Option<String>, flagged: Option<String>) -> Result<Option<String>, &'static str> {
    match flagged {
        Some(flagged) if set.as_ref() != Some(&flagged) => Err("deployment_conflict"),
        _ => Ok(set),
    }
}

/// Whether `--fixture`, the in-memory journal, is allowed: never on a pod the deployment manager
/// started (`MANDATE_PROCESS` set), which must keep its journal, so it never silently runs on one
/// that forgets everything at exit (`AGENTS.md` rule 3).
///
/// # Errors
/// `fixture_under_manager` for `--fixture` on a pod the manager started.
pub fn fixture_permitted(
    fixture: bool,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<(), &'static str> {
    if fixture && var("MANDATE_PROCESS").is_some() {
        return Err("fixture_under_manager");
    }
    Ok(())
}

/// The model-gateway service's address `run` was given with `--gateway <host:port>` (DEC-2397 item
/// 6), parsed and never resolved here, since [`crate::remote::RemoteGateway`] resolves it for each
/// call; or `None` when the flag is absent, so the runtime keeps its in-process recorded fake
/// (DEC-2257 item 6). The flag is read only beside `--research`, since only research calls a model.
///
/// # Errors
/// `gateway_address_invalid` for the flag written `--gateway=`, with no value, with a value that is
/// another flag or does not parse as `host:port`, given twice, or without `--research`.
pub fn gateway(args: &[String], research: bool) -> Result<Option<GatewayAddress>, &'static str> {
    let joined = args.iter().any(|a| a.starts_with("--gateway="));
    let Some(at) = args.iter().position(|a| a == "--gateway") else {
        return if joined {
            Err("gateway_address_invalid")
        } else {
            Ok(None)
        };
    };
    let twice = args.iter().filter(|a| *a == "--gateway").nth(1).is_some();
    if joined || twice || !research {
        return Err("gateway_address_invalid");
    }
    args.get(at.saturating_add(1))
        .filter(|value| !value.starts_with("--"))
        .and_then(|value| GatewayAddress::parse(value))
        .map(Some)
        .ok_or("gateway_address_invalid")
}
