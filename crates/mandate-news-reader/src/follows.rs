//! The agents the reader follows (DEC-2278 item 2, DEC-2280 item 1): each an agent id and the
//! broker account its executor trades. The reader reads only snapshot rows of the control stream,
//! so it cannot find its agents there; the deployment names them, `--follow <agent>=<account>`.
//! Every follow is checked at start: each must name a valid agent stream and account stream of the
//! workspace, and there must be one, or the reader is not ready (`follow_malformed`,
//! `no_follows`) rather than polling some of them and silently not the rest.

use mandate_journal::StreamId;

/// One agent the reader follows, as the deployment names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follow {
    pub agent: String,
    pub account: String,
}

/// A follow checked against the workspace: the agent and the streams the reader reads for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Followed {
    pub agent: String,
    pub agent_stream: StreamId,
    pub account_stream: StreamId,
}

/// Each `--follow <value>` in `args`, in order, as written: `<agent>=<account>`, and a value with no
/// `=`, or a flag with no value, as a follow with an empty side, which [`check`] refuses.
pub fn parse_follows(args: &[String]) -> Vec<Follow> {
    args.iter()
        .enumerate()
        .filter(|(_, flag)| *flag == "--follow")
        .map(|(at, _)| {
            let value = at.checked_add(1).and_then(|next| args.get(next));
            let (agent, account) = value
                .and_then(|value| value.split_once('='))
                .unwrap_or_default();
            Follow {
                agent: agent.to_owned(),
                account: account.to_owned(),
            }
        })
        .collect()
}

/// `follows` checked against `workspace`.
///
/// # Errors
/// `no_follows` for none, and `follow_malformed` for any whose agent or account does not make a
/// valid stream id of the workspace.
pub fn check(workspace: &str, follows: &[Follow]) -> Result<Vec<Followed>, &'static str> {
    if follows.is_empty() {
        return Err("no_follows");
    }
    follows
        .iter()
        .map(|follow| {
            let agent_stream = StreamId::parse(&format!("agent:{workspace}:{}", follow.agent));
            let account_stream = StreamId::parse(&format!("acct:{workspace}:{}", follow.account));
            match (agent_stream, account_stream) {
                (Some(agent_stream), Some(account_stream)) => Ok(Followed {
                    agent: follow.agent.clone(),
                    agent_stream,
                    account_stream,
                }),
                _ => Err("follow_malformed"),
            }
        })
        .collect()
}
