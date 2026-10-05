//! `mandate-tracer`: one pass of the tracer bullet on the owner's Alpaca **paper** account
//! ([task brief](../../../../docs/project/tasks/E7-7-tracer-bullet.md), backlog E7-7).
//!
//! ```text
//! mandate-tracer --mandate <path> --dataset <dir> --confirm-paper
//! mandate-tracer --mandate <path> --dataset <dir> --journal <dsn> --confirm-paper --place-one-order
//! ```
//!
//! A refusal exits non-zero and prints its stable reason code and message on one line of stderr.
//! Until the exit probes are real every run refuses with `exit_path_unavailable`, before any
//! request, so the binary holds no transport and reads no credential yet (DEC-166 item 3).

use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};
use mandate_shell::adapters::{Disconnected, Sources, over};
use mandate_shell::{Report, Setup, ShellError, cli, host, run};
use mandate_time::UtcNanos;

fn main() -> ExitCode {
    match tracer() {
        Ok(report) => {
            if let Some(order) = &report.would_place {
                println!(
                    "would place {} (nothing sent; pass --place-one-order)",
                    order.client_order_id.as_str()
                );
            }
            for id in &report.submitted {
                println!("submitted {id}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

fn tracer() -> Result<Report, ShellError> {
    let args = cli::parse(std::env::args().skip(1))?;
    host::refuse_configured_host(std::env::vars_os().map(|(name, value)| {
        (
            name.to_string_lossy().into_owned(),
            value.to_string_lossy().into_owned(),
        )
    }))?;
    let recorded_at = now()?;
    let setup = Setup {
        deployment: Deployment {
            agent: AgentId(cli::AGENT.to_owned()),
            connection: ConnectionId(cli::CONNECTION.to_owned()),
            workspace: WorkspaceId(cli::WORKSPACE.to_owned()),
        },
        account_ref: cli::ACCOUNT_REF.to_owned(),
        now: recorded_at,
        place_one_order: args.place_one_order,
        new_cycle: args.new_cycle,
    };
    let mut stages = over(Sources {
        mandate: args.mandate,
        dataset: args.dataset,
        journal: args.journal,
        recorded_at,
        agent: AgentId(cli::AGENT.to_owned()),
        transport: Box::new(Disconnected),
    });
    run(&mut stages, &setup)
}

/// The process's one clock reading, injected into the library as an input (ADR-0001 ES-05).
#[allow(
    clippy::disallowed_methods,
    reason = "the binary is the shell's one clock reader; the library takes time as an input (ES-05)"
)]
fn now() -> Result<UtcNanos, ShellError> {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ShellError::Usage("the system clock is before 1970".to_owned()))?;
    let secs = i64::try_from(since.as_secs())
        .map_err(|_| ShellError::Usage("the system clock is out of range".to_owned()))?;
    UtcNanos::from_parts(secs, 0)
        .map_err(|_| ShellError::Usage("the system clock is out of range".to_owned()))
}
