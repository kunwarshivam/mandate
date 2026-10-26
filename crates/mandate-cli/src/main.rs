use std::io::{self, IsTerminal};
use std::time::SystemTime;

use anyhow::{Context, bail};
use clap::Parser;
use mandate_cli::artifact::ArtifactCommand;
use mandate_cli::backtest::BacktestCommand;
use mandate_cli::brake::AgentCommand;
use mandate_cli::connection::ConnectionCommand;
use mandate_cli::control::Now;
use mandate_cli::db::DbCommand;
use mandate_cli::disclosure::DisclosureCommand;
use mandate_cli::evaluation::EvaluationCommand;
use mandate_cli::gestures::VersionCommand;
use mandate_cli::identity::IdentityCommand;
use mandate_cli::inbox::ApprovalsCommand;
use mandate_cli::journal::JournalCommand;
use mandate_cli::postgres::PgControlJournal;
use mandate_cli::register::{ConfigCommand, ModelCommand};
#[cfg(feature = "rh-sim")]
use mandate_cli::sim_clock::from_env;
use mandate_cli::step_up::StepUpCommand;
use mandate_cli::vault::VaultCommand;
use mandate_cli::workspace::WorkspaceCommand;
use mandate_cli::{
    Cli, Command, artifact, backtest, brake, connection, db, disclosure, download, evaluation,
    gestures, identity, inbox, inspect, journal, owner, register, step_up, vault, workspace,
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
    let system = Now { at, secs };
    #[cfg(feature = "rh-sim")]
    let system = from_env(system)?;
    Ok(system)
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
        Command::Workspace(WorkspaceCommand::Found(args)) => {
            let mut last = now()?.at;
            let mut clock = move || {
                last = now().map_or(last, |n| n.at);
                last
            };
            workspace::found(&args, &mut clock, &mut io::stdout().lock()).map(drop)
        }
        Command::Identity(IdentityCommand::Link(args)) => {
            let linked = identity::link(&args, &mut io::stdin().lock(), &mut io::stdout().lock());
            linked.map(drop).map_err(Into::into)
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
        Command::Agent(AgentCommand::Gesture(gestures::AgentCommand::Deploy(args))) => {
            gestures::deploy(&args, now()?, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Agent(AgentCommand::Pause(args)) => {
            brake::run_pause(&args, now()?, &mut io::stdout().lock())
        }
        Command::Agent(AgentCommand::Kill(args)) => {
            brake::run_kill(&args, now()?, &mut io::stdout().lock()).map(drop)
        }
        Command::Agent(AgentCommand::Status(args)) => {
            owner::run_status(&args, &mut io::stdout().lock())
        }
        Command::Agent(AgentCommand::Resume(args)) => {
            owner::run_resume(&args, now()?, &mut io::stdout().lock())
        }
        Command::Agent(AgentCommand::Stop(args)) => {
            owner::run_stop(&args, now()?, &mut io::stdout().lock())
        }
        Command::Agent(AgentCommand::Exit(args)) => {
            owner::run_exit(&args, now()?, &mut io::stdout().lock())
        }
        Command::Agent(AgentCommand::Acknowledge(args)) => {
            owner::run_acknowledge(&args, now()?, &mut io::stdout().lock())
        }
        Command::Agent(AgentCommand::RequestAck(args)) => {
            owner::run_request_ack(&args, now()?, &mut io::stdout().lock())
        }
        Command::Vault(VaultCommand::Import(args)) => {
            let environment: Vec<(String, String)> = std::env::vars_os()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            vault::run_import(
                &args,
                &environment,
                &vault::UserDatabase::from_host()?,
                &mut io::stdin().lock(),
                &mut io::stdout().lock(),
            )
        }
        Command::Vault(VaultCommand::Put(args)) => vault::run_put(
            &args,
            &mut io::stdin().lock(),
            vault::Terminals {
                stdin: io::stdin().is_terminal(),
                stdout: io::stdout().is_terminal(),
            },
            now()?.at,
            &mut io::stdout().lock(),
        ),
        Command::Vault(VaultCommand::Provision(args)) => vault::run_provision(
            &args,
            &mut io::stdin().lock(),
            vault::Terminals {
                stdin: io::stdin().is_terminal(),
                stdout: io::stdout().is_terminal(),
            },
            &mut io::stdout().lock(),
        ),
        Command::Db(DbCommand::Migrate(args)) => {
            db::migrate(&args, &mut io::stdout().lock())?;
            Ok(())
        }
        Command::Backtest(BacktestCommand::Record(args)) => {
            backtest::run_record(&args, now()?, &mut io::stdout().lock()).map(drop)
        }
        Command::Evaluation(EvaluationCommand::Run(args)) => {
            evaluation::run_command(&args, now()?, &mut io::stdout().lock()).map(drop)
        }
        Command::StepUp(StepUpCommand::Confirm(args)) => {
            let checked = step_up::check(&(&args).into())?;
            let mut journal = PgControlJournal::open(&args.target)?;
            let out = &mut io::stdout().lock();
            step_up::run_confirm(&checked, &mut journal, now()?, out).map(drop)
        }
        Command::StepUp(StepUpCommand::ConfirmConnect(args)) => {
            step_up::run_connect_command(&args, now()?, &mut io::stdout().lock()).map(drop)
        }
        Command::Disclosure(DisclosureCommand::Accept(args)) => {
            disclosure::run(&args, now()?, &mut io::stdout().lock()).map(drop)
        }
    }
}
