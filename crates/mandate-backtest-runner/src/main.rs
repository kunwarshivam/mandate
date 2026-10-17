//! `mandate-backtest-runner run`: reads the five input files, runs the backtest, and prints the
//! backtest id, the payload's reference, and the report's or the refusal's code; the logic is the
//! library's ([`mandate_backtest_runner::run`]).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_backtest_runner::{RunnerError, read_dir, run};

/// Runs a mandate version's backtest and stores its report and record payload as artifacts.
#[derive(Debug, Parser)]
#[command(name = "mandate-backtest-runner", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one backtest. `--inputs` holds five JSON files (DEC-1144 item 3): `bars.json`
    /// {coverage_start, corporate_actions, bars: [{start, open, high, low, close, volume,
    /// trade_date, session, session_start, auction, first_bar_median}]}; `outputs.json` {outputs:
    /// [{received_at, model_id, model_version, content_hash, instrument, as_of, expires_at,
    /// direction, conviction, confidence}]}; `theses.json` {theses: [{thesis_id, received_at,
    /// instrument, as_of, expires_at, admitted, confidence, invalidated_at}]}; `instrument.json`,
    /// the instrument reference; and `settings.json` {sim, fees, gate, profile, account_type,
    /// regime, metrics, min_order_usd, mark_max_age_s, benchmark_collar_bps}. Decimals are
    /// strings, counts integers, instants RFC 3339 UTC; every member is required and an unknown
    /// one is refused. Prints `backtest_id=`, `payload=` and `report=` or `refused=` lines; record
    /// the payload with `mandate backtest record`.
    Run {
        /// The directory holding the five input files.
        #[arg(long, value_name = "DIR")]
        inputs: PathBuf,
        /// The artifact store's root, which holds the mandate document and takes the outputs.
        #[arg(long, value_name = "DIR")]
        store: PathBuf,
        /// The confirmed mandate version to backtest, `sha256:` and 64 hex digits.
        #[arg(long, value_name = "REF")]
        mandate_version: String,
        /// The backtest's id: 1 to 64 letters, digits, `_` or `-`.
        #[arg(long, value_name = "ID")]
        backtest_id: String,
    },
}

fn main() -> ExitCode {
    let Command::Run {
        inputs,
        store,
        mandate_version,
        backtest_id,
    } = Cli::parse().command;
    let ran = read_dir(&inputs).and_then(|decoded| {
        let mut store = FsArtifactStore::open(&store).map_err(|e| RunnerError::Store(e.code()))?;
        run(&mut store, &backtest_id, &mandate_version, &decoded)
    });
    match ran {
        Ok(ran) => {
            print!("{ran}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("refused ({}): {error}", error.code());
            ExitCode::FAILURE
        }
    }
}
