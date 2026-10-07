use std::io;
use std::time::SystemTime;

use anyhow::{Context, bail};
use clap::Parser;
use mandate_cli::artifact::ArtifactCommand;
use mandate_cli::journal::JournalCommand;
use mandate_cli::{Cli, Command, artifact, download, inspect, journal};
use mandate_marketdata::client::{Client, TokioPause};
use mandate_marketdata::http::{AlpacaDataHttp, Credentials};
use mandate_time::{Date, UtcNanos};

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the wall clock once, to refuse days that are not over yet (ES-05)"
)]
fn today_utc() -> anyhow::Result<Date> {
    let since_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("the system clock is before 1970")?;
    let secs = i64::try_from(since_epoch.as_secs()).context("the system clock is out of range")?;
    Ok(UtcNanos::from_parts(secs, 0)?.date())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Download(args) => {
            let plan = download::plan(&args, today_utc()?)?;
            let http = AlpacaDataHttp::new(Credentials::from_env()?)?;
            let client = Client::new(http, TokioPause);
            download::run(&plan, &client, &mut io::stdout().lock()).await?;
            Ok(())
        }
        Command::Inspect(args) => {
            let problems = inspect::run(&args, &mut io::stdout().lock())?;
            if problems > 0 {
                bail!("{problems} problems found");
            }
            Ok(())
        }
        Command::Journal(JournalCommand::Verify(args)) => {
            let outcome = journal::verify(&args, &mut io::stdout().lock())?;
            if outcome.failed() {
                bail!("{outcome}");
            }
            Ok(())
        }
        Command::Journal(JournalCommand::VerifyCold(args)) => {
            let outcome = journal::cold::verify(&args, &mut io::stdout().lock())?;
            if outcome.failed() {
                bail!("{outcome}");
            }
            Ok(())
        }
        Command::Artifact(ArtifactCommand::Put(args)) => {
            artifact::put(&args, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Artifact(ArtifactCommand::Get(args)) => {
            artifact::get(&args, &mut io::stdout().lock())?;
            Ok(())
        }
    }
}
