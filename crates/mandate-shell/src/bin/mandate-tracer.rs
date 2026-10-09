//! `mandate-tracer`: one pass of the tracer bullet on the owner's Alpaca **paper** account
//! ([task brief](../../../../docs/project/tasks/E7-7-tracer-bullet.md), backlog E7-7).
//!
//! ```text
//! mandate-tracer --mandate <path> --dataset <dir> --config-dir <dir>
//!   --workspace <id> --agent <id> --account-ref <id> --confirm-paper
//! mandate-tracer --mandate <path> --dataset <dir> --config-dir <dir>
//!   --workspace <id> --agent <id> --account-ref <id> --journal <dsn>
//!   --confirm-paper --place-one-order
//! ```
//!
//! A refusal exits non-zero and prints its stable reason code and message on one line of stderr.
//!
//! The order of a run is fixed (DEC-466, DEC-470): the host refusal and the reviewed artifacts
//! first, which read no credential; then the paper credentials and the one transport; then the
//! GET-only preflight of the account, positions, open orders, asset record, latest IEX quote, and
//! the last five minutes' IEX minute bars (DEC-471); then the liquidity facts, from the stored daily
//! bars and those minute bars; and only then the trusted contexts, assembled from that one
//! snapshot, and the run, whose executor journals the intent before its one `POST`.

use std::process::ExitCode;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use mandate_alpaca::{AlpacaPaperHttp, Credentials, TokioPause};
use mandate_shell::adapters::{Sources, production};
use mandate_shell::paper::{
    Artifacts, PaperClock, PaperFacts, liquidity_facts, load_contexts_with_clock, preflight,
};
use mandate_shell::{Report, Setup, ShellError, Stage, cli, host, run};
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
    let args = cli::parse_production(std::env::args().skip(1))?;
    host::refuse_configured_host(std::env::vars_os().map(|(name, value)| {
        (
            name.to_string_lossy().into_owned(),
            value.to_string_lossy().into_owned(),
        )
    }))?;
    let artifacts =
        Artifacts::load_production(&args.mandate, &args.config_dir).map_err(|cause| {
            ShellError::Refused {
                stage: Stage::Validate,
                cause,
            }
        })?;
    let deployment_input = artifacts.deployment(
        args.workspace.clone(),
        args.agent.clone(),
        args.account_ref.clone(),
    );
    let deployment_input = deployment_input.map_err(|cause| ShellError::Refused {
        stage: Stage::Validate,
        cause,
    })?;
    let credentials = Credentials::from_env()
        .map_err(|_| ShellError::Usage("Alpaca paper credentials are unavailable".to_owned()))?;
    let transport = AlpacaPaperHttp::new(credentials)
        .map_err(|_| ShellError::Usage("the Alpaca paper client is unavailable".to_owned()))?;
    let broker = preflight(&artifacts, transport.clone(), transport.clone(), TokioPause).map_err(
        |cause| ShellError::Refused {
            stage: Stage::Reconcile,
            cause,
        },
    )?;
    let recorded_at = now()?;
    let liquidity = liquidity_facts(
        artifacts.production_identity().symbol,
        &args.dataset,
        &broker.minute_bars,
        recorded_at,
    )
    .map_err(|cause| ShellError::Refused {
        stage: Stage::MarketData,
        cause,
    })?;
    let agent = deployment_input.deployment().agent.clone();
    let contexts = load_contexts_with_clock(
        &artifacts,
        &PaperFacts { broker, liquidity },
        recorded_at,
        &agent,
        Rc::new(SystemClock),
    )
    .map_err(|cause| ShellError::Refused {
        stage: Stage::Validate,
        cause,
    })?;
    let setup = Setup {
        deployment: deployment_input.deployment().clone(),
        account_ref: deployment_input.account_ref().to_owned(),
        now: recorded_at,
        place_one_order: args.place_one_order,
        new_cycle: false,
    };
    let mut stages = production(Sources {
        mandate: args.mandate,
        dataset: args.dataset,
        journal: args.journal,
        recorded_at,
        agent,
        workspace: deployment_input.deployment().workspace.0.clone(),
        account_ref: deployment_input.account_ref().to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: None,
        transport,
    });
    run(&mut stages, &setup)
}

struct SystemClock;

impl PaperClock for SystemClock {
    fn now(&self) -> Option<UtcNanos> {
        match now() {
            Ok(now) => Some(now),
            Err(error) => {
                let _ = error;
                None
            }
        }
    }
}

/// The process's clock source, injected into the library (ADR-0001 ES-05). It keeps nanoseconds,
/// so quote ages are not rounded in their favour; the binding gate reads it immediately before
/// submission.
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
    UtcNanos::from_parts(secs, since.subsec_nanos())
        .map_err(|_| ShellError::Usage("the system clock is out of range".to_owned()))
}

#[cfg(test)]
mod tests {
    use mandate_shell::paper::PaperClock;

    use super::SystemClock;

    #[test]
    fn the_shipping_clock_reads_a_current_instant() {
        assert!(SystemClock.now().is_some());
    }
}
