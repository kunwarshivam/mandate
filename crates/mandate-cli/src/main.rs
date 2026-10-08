use std::io;
use std::time::SystemTime;

use anyhow::{Context, bail};
use clap::Parser;
use mandate_cli::artifact::ArtifactCommand;
use mandate_cli::connection::ConnectionCommand;
use mandate_cli::control::Now;
use mandate_cli::gestures::{AgentCommand, VersionCommand};
use mandate_cli::inbox::ApprovalsCommand;
use mandate_cli::journal::JournalCommand;
use mandate_cli::register::{ConfigCommand, ModelCommand};
use mandate_cli::workspace::WorkspaceCommand;
use mandate_cli::{
    Cli, Command, artifact, connection, download, gestures, inbox, inspect, journal, register,
    workspace,
};
use mandate_marketdata::client::{Client, TokioPause};
use mandate_marketdata::http::{AlpacaDataHttp, Credentials};
use mandate_time::{Date, UtcNanos};

/// The day the owner runs the command, to refuse days that are not over yet.
fn today_utc() -> anyhow::Result<Date> {
    Ok(now()?.at.date())
}

#[allow(
    clippy::disallowed_methods,
    reason = "the binary's edge reads the wall clock once, for the moment the owner ran a command (ES-05)"
)]
fn now() -> anyhow::Result<Now> {
    let since_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("the system clock is before 1970")?;
    let secs = i64::try_from(since_epoch.as_secs()).context("the system clock is out of range")?;
    let at = UtcNanos::from_parts(secs, since_epoch.subsec_nanos())?;
    Ok(Now { at, secs })
}

/// Synchronous, so a command that blocks on its journal's own runtime never runs inside another
/// (DEC-522); `download` builds the one runtime it needs.
fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Download(args) => {
            let plan = download::plan(&args, today_utc()?)?;
            let http = AlpacaDataHttp::new(Credentials::from_env()?)?;
            let client = Client::new(http, TokioPause);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("starting the download's runtime")?;
            runtime.block_on(download::run(&plan, &client, &mut io::stdout().lock()))?;
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
        Command::Journal(JournalCommand::Export(args)) => {
            journal::export::export(&args, &mut io::stdout().lock())?;
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
        Command::Approvals(command) => {
            let out = &mut io::stdout().lock();
            match command {
                ApprovalsCommand::List(args) => inbox::run_list(&args, now()?, out).map(drop),
                ApprovalsCommand::Show(args) => inbox::run_show(&args, out).map(drop),
                ApprovalsCommand::Approve(args) => inbox::run_approve(&args, now()?, out).map(drop),
                ApprovalsCommand::Skip(args) => inbox::run_skip(&args, now()?, out).map(drop),
            }
        }
        Command::Config(ConfigCommand::Register(args)) => {
            register::run(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Model(ModelCommand::Register(args)) => {
            register::run_model(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Workspace(WorkspaceCommand::Open(args)) => {
            workspace::open(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Connection(ConnectionCommand::Record(args)) => {
            connection::run(&args, now()?, &mut io::stdout().lock()).map(drop)
        }
        Command::Version(VersionCommand::Create(args)) => {
            gestures::create(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Version(VersionCommand::Confirm(args)) => {
            gestures::confirm(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Agent(AgentCommand::Deploy(args)) => {
            gestures::deploy(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
    }
}
