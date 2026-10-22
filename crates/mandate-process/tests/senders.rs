//! Only the executor exits `Completed` (DEC-2347 item 2): every daemon shares this lifecycle, so
//! no crate's sources but `mandate-executord`'s may name `Input::Completed`, under whatever name
//! they import the lifecycle's `Input` as.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, at any depth.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The names `text` gives the lifecycle's `Input`: each `use mandate_process` statement's
/// `Input` or `Input as <name>`, and the paths that reach it without a `use`. A glob import of
/// the crate or its lifecycle names `Input` itself.
fn names_of_input(text: &str) -> Vec<String> {
    let mut names = vec![
        "mandate_process::Input".to_owned(),
        "mandate_process::lifecycle::Input".to_owned(),
    ];
    for statement in text.split("use mandate_process").skip(1) {
        let statement = statement.split(';').next().unwrap_or_default();
        let words: Vec<&str> = statement
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '*'))
            .filter(|word| !word.is_empty())
            .collect();
        for (at, word) in words.iter().enumerate() {
            match (*word, words.get(at + 1), words.get(at + 2)) {
                ("Input", Some(&"as"), Some(alias)) => names.push((*alias).to_owned()),
                ("Input" | "*", _, _) => names.push("Input".to_owned()),
                _ => {}
            }
        }
    }
    names
}

/// Each crate under `crates/` whose sources name `Input::Completed`.
fn senders() -> Vec<String> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut senders = Vec::new();
    let Ok(entries) = fs::read_dir(&crates) else {
        panic!("the workspace's crates read");
    };
    for entry in entries.flatten() {
        let mut files = Vec::new();
        sources(&entry.path().join("src"), &mut files);
        let sends = files.iter().any(|file| {
            let text = fs::read_to_string(file).unwrap_or_default();
            names_of_input(&text).iter().any(|name| {
                text.split(&format!("{name}::Completed"))
                    .skip(1)
                    .any(|after| !after.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            })
        });
        if sends {
            senders.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    senders.sort();
    senders
}

#[test]
#[ignore = "pending E7-4"]
fn only_the_executor_sends_completed() {
    assert_eq!(senders(), ["mandate-executord"]);
}

/// The reading itself: an alias, a bare import, a glob, and a full path are each caught, and a
/// name another crate gives its own `Input` is not.
#[test]
fn every_name_for_the_input_is_read() {
    let caught = [
        "use mandate_process::{Input as Lifecycled, Kind};\nLifecycled::Completed",
        "use mandate_process::Input;\nInput::Completed",
        "use mandate_process::lifecycle::{\n    Exit,\n    Input,\n};\nInput::Completed;",
        "use mandate_process::*;\nInput::Completed",
        "mandate_process::Input::Completed",
    ];
    let names = |text: &str| names_of_input(text);
    for text in caught {
        assert!(
            names(text)
                .iter()
                .any(|name| text.contains(&format!("{name}::Completed"))),
            "{text}"
        );
    }
    let missed = "use mandate_process::{Input as Lifecycled};\nuse other::Input;\nInput::Completed";
    assert!(
        !names(missed)
            .iter()
            .any(|name| missed.contains(&format!("{name}::Completed"))),
    );
}
