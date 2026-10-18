//! `mandate evaluation run`'s command line (DEC-2244 item 1): the flags it takes, the paper owner
//! with no user, the inputs directory, the cut-off, and what it prints. Each refuses before the
//! journal is opened.

use std::path::PathBuf;

use clap::Parser;
use mandate_cli::control::{ControlError, Submitted};
use mandate_cli::evaluation::{
    Evaluated, EvaluationCommand, cut_off, evaluator_owner, print_evaluated, read_inputs,
};
use mandate_cli::{Cli, Command};
use mandate_journal::Environment;
use mandate_time::UtcNanos;

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// The flags parse into the command, with `--artifacts` the store's alias.
#[test]
#[ignore = "pending E17-8"]
fn the_command_takes_its_flags() {
    let cli = Cli::try_parse_from([
        "mandate",
        "evaluation",
        "run",
        "--workspace",
        "ws1",
        "--inputs",
        "/in",
        "--evaluation-id",
        "ev_1",
        "--cut-off",
        "2026-09-24T00:00:00Z",
        "--journal",
        "postgres://journal.invalid/j",
        "--artifacts",
        "/store",
    ])
    .unwrap_or_else(|e| panic!("{e}"));
    let Command::Evaluation(EvaluationCommand::Run(args)) = cli.command else {
        panic!("evaluation run");
    };
    assert_eq!(args.workspace, "ws1");
    assert_eq!(args.inputs, PathBuf::from("/in"));
    assert_eq!(args.evaluation_id, "ev_1");
    assert_eq!(args.cut_off, "2026-09-24T00:00:00Z");
    assert_eq!(args.store, Some(PathBuf::from("/store")));
    for unknown in ["--environment", "--user"] {
        assert!(
            Cli::try_parse_from([
                "mandate",
                "evaluation",
                "run",
                "--workspace",
                "ws1",
                "--inputs",
                "/in",
                "--evaluation-id",
                "ev_1",
                "--cut-off",
                "x",
                "--journal",
                "j",
                unknown,
                "live",
            ])
            .is_err(),
            "{unknown}"
        );
    }
    let owner = evaluator_owner("ws1").unwrap();
    assert_eq!(
        (
            owner.workspace.as_str(),
            owner.user.as_str(),
            owner.environment
        ),
        ("ws1", "", Environment::Paper)
    );
    assert_eq!(
        evaluator_owner("ws 1"),
        Err(refused("owner_workspace_invalid"))
    );
}

/// The four files are read in their order; a missing one refuses `input_invalid`.
#[test]
#[ignore = "pending E17-8"]
fn the_inputs_are_the_four_files_in_order() {
    let dir = std::env::temp_dir().join(format!("mandate-cli-eval-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let names = [
        "decision.json",
        "closes.json",
        "cost.json",
        "baselines.json",
    ];
    for (i, name) in names.iter().enumerate() {
        std::fs::write(dir.join(name), format!("{{\"file\":{i}}}")).unwrap();
    }
    let read = read_inputs(&dir).unwrap();
    for (i, bytes) in read.iter().enumerate() {
        assert_eq!(bytes, format!("{{\"file\":{i}}}").as_bytes());
    }
    std::fs::remove_file(dir.join("cost.json")).unwrap();
    assert_eq!(read_inputs(&dir), Err(refused("input_invalid")));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(read_inputs(&dir), Err(refused("input_invalid")));
}

/// The cut-off is RFC 3339 UTC, to the nanosecond; anything else refuses `cut_off_invalid`.
#[test]
#[ignore = "pending E17-8"]
fn the_cut_off_is_an_rfc_3339_instant() {
    assert_eq!(
        cut_off("2026-09-24T00:00:00.000000001Z"),
        Ok(UtcNanos::from_parts(1_790_208_000, 1).unwrap())
    );
    for bad in ["", "2026-09-24", "yesterday", "1790208000"] {
        assert_eq!(cut_off(bad), Err(refused("cut_off_invalid")), "{bad}");
    }
}

/// What it prints: the id, the report, whether it passed, and the event.
#[test]
#[ignore = "pending E17-8"]
fn it_prints_the_evaluation_and_its_event() {
    let mut out = Vec::new();
    let evaluated = Evaluated {
        evaluation_id: "ev_1".to_owned(),
        report: "sha256:aa".to_owned(),
        passed: false,
        submitted: Submitted {
            event_id: "01J8ZA00000000000000000001".to_owned(),
            seq: 7,
        },
    };
    print_evaluated(&mut out, &evaluated).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "evaluation_id=ev_1\nreport=sha256:aa\npassed=false\nrecorded as event 01J8ZA00000000000000000001 at seq 7\n"
    );
}
