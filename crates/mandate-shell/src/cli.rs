//! The `mandate-tracer` command line (task brief, "The manual paper run").
//!
//! Nothing is sent without `--place-one-order`, and nothing runs without `--confirm-paper`: two
//! separate acknowledgements, both required to submit. There is no `--host`, so any attempt to
//! name one is an unknown argument.

use std::path::PathBuf;

use crate::error::ShellError;

/// The workspace the tracer's streams belong to: the team's internal paper workspace (DEC-103).
pub const WORKSPACE: &str = "tracer";
/// The tracer's agent deployment.
pub const AGENT: &str = "tracer-aapl";
/// The paper connection the mandate names.
pub const CONNECTION: &str = "conn_alpaca_paper_01";
/// The account stream's opaque subject, never the broker's account number (TI-8).
pub const ACCOUNT_REF: &str = "tracer-paper";

/// What the operator asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub mandate: PathBuf,
    pub dataset: PathBuf,
    /// Effective-dated E7-7 artifacts, including the pinned model artifact.
    pub config_dir: PathBuf,
    /// The journal's DSN. Only a run that places its order keeps a journal: a planning run uses a
    /// scratch one, so a later start cannot re-hand what it proposed (DEC-157 item 6).
    pub journal: Option<String>,
    pub place_one_order: bool,
    pub new_cycle: bool,
}

/// Parses the arguments after the program name.
///
/// # Errors
/// [`ShellError::Usage`] for a missing `--confirm-paper`, an unknown argument, a flag without its
/// value, or a journal on a run that places nothing.
pub fn parse<I>(args: I) -> Result<Args, ShellError>
where
    I: IntoIterator<Item = String>,
{
    let mut mandate = None;
    let mut dataset = None;
    let mut config_dir = None;
    let mut journal = None;
    let mut confirm_paper = false;
    let mut place_one_order = false;
    let mut new_cycle = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mandate" => mandate = Some(PathBuf::from(value_of(&arg, args.next())?)),
            "--dataset" => dataset = Some(PathBuf::from(value_of(&arg, args.next())?)),
            "--config-dir" => config_dir = Some(PathBuf::from(value_of(&arg, args.next())?)),
            "--journal" => journal = Some(value_of(&arg, args.next())?),
            "--confirm-paper" => confirm_paper = true,
            "--place-one-order" => place_one_order = true,
            "--new-cycle" => new_cycle = true,
            unknown => return Err(usage(format!("unknown argument {unknown}"))),
        }
    }
    if !confirm_paper {
        return Err(usage("--confirm-paper is required".to_owned()));
    }
    if journal.is_some() && !place_one_order {
        return Err(usage(
            "a run without --place-one-order keeps no journal; drop --journal".to_owned(),
        ));
    }
    if journal.is_none() && place_one_order {
        return Err(usage(
            "--place-one-order needs --journal, so the order is on the record".to_owned(),
        ));
    }
    Ok(Args {
        mandate: mandate.ok_or_else(|| usage("--mandate is required".to_owned()))?,
        dataset: dataset.ok_or_else(|| usage("--dataset is required".to_owned()))?,
        config_dir: config_dir.ok_or_else(|| usage("--config-dir is required".to_owned()))?,
        journal,
        place_one_order,
        new_cycle,
    })
}

fn value_of(flag: &str, value: Option<String>) -> Result<String, ShellError> {
    match value {
        Some(value) if !value.starts_with("--") => Ok(value),
        Some(_) | None => Err(usage(format!("{flag} needs a value"))),
    }
}

fn usage(message: String) -> ShellError {
    ShellError::Usage(message)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Args, parse};
    use crate::error::ShellError;

    fn args(words: &[&str]) -> Result<Args, ShellError> {
        parse(words.iter().map(|w| (*w).to_owned()))
    }

    fn usage_of(words: &[&str]) -> String {
        match args(words) {
            Err(ShellError::Usage(message)) => message,
            Ok(parsed) => format!("parsed {parsed:?}"),
            Err(other) => format!("{other:?}"),
        }
    }

    #[test]
    fn a_planning_run_needs_the_paper_acknowledgement_and_keeps_no_journal() -> Result<(), String> {
        let parsed = args(&[
            "--mandate",
            "m.json",
            "--dataset",
            "bars",
            "--config-dir",
            "config",
            "--confirm-paper",
        ])
        .map_err(|e| e.to_string())?;
        assert_eq!(
            parsed,
            Args {
                mandate: PathBuf::from("m.json"),
                dataset: PathBuf::from("bars"),
                config_dir: PathBuf::from("config"),
                journal: None,
                place_one_order: false,
                new_cycle: false,
            }
        );
        assert_eq!(
            usage_of(&["--mandate", "m.json", "--dataset", "bars"]),
            "--confirm-paper is required"
        );
        assert_eq!(
            usage_of(&[
                "--mandate",
                "m.json",
                "--dataset",
                "bars",
                "--journal",
                "pg",
                "--confirm-paper"
            ]),
            "a run without --place-one-order keeps no journal; drop --journal"
        );
        Ok(())
    }

    #[test]
    fn placing_an_order_needs_both_flags_and_a_journal() -> Result<(), String> {
        let parsed = args(&[
            "--confirm-paper",
            "--place-one-order",
            "--new-cycle",
            "--journal",
            "postgres://localhost/j",
            "--config-dir",
            "config",
            "--mandate",
            "m.json",
            "--dataset",
            "bars",
        ])
        .map_err(|e| e.to_string())?;
        assert!(parsed.place_one_order);
        assert!(parsed.new_cycle);
        assert_eq!(parsed.config_dir, PathBuf::from("config"));
        assert_eq!(parsed.journal.as_deref(), Some("postgres://localhost/j"));
        assert_eq!(
            usage_of(&[
                "--mandate",
                "m.json",
                "--dataset",
                "bars",
                "--place-one-order"
            ]),
            "--confirm-paper is required"
        );
        assert_eq!(
            usage_of(&[
                "--mandate",
                "m.json",
                "--dataset",
                "bars",
                "--confirm-paper",
                "--place-one-order"
            ]),
            "--place-one-order needs --journal, so the order is on the record"
        );
        Ok(())
    }

    #[test]
    fn there_is_no_host_argument_and_every_flag_needs_its_value() {
        assert_eq!(
            usage_of(&["--confirm-paper", "--host", "https://api.alpaca.markets"]),
            "unknown argument --host"
        );
        assert_eq!(
            usage_of(&["--confirm-paper", "--mandate"]),
            "--mandate needs a value"
        );
        assert_eq!(
            usage_of(&["--mandate", "--confirm-paper", "--dataset", "bars"]),
            "--mandate needs a value"
        );
        assert_eq!(
            usage_of(&["--confirm-paper", "--dataset", "bars"]),
            "--mandate is required"
        );
        assert_eq!(
            usage_of(&["--confirm-paper", "--mandate", "m.json"]),
            "--dataset is required"
        );
        assert_eq!(
            usage_of(&[
                "--confirm-paper",
                "--mandate",
                "m.json",
                "--dataset",
                "bars"
            ]),
            "--config-dir is required"
        );
        assert_eq!(
            usage_of(&["--confirm-paper", "--minute-dataset", "minutes"]),
            "unknown argument --minute-dataset",
            "the trailing volume is read from the broker, never from a stored dataset (DEC-471)"
        );
        assert_eq!(
            usage_of(&["--confirm-paper", "--journal"]),
            "--journal needs a value"
        );
    }
}
