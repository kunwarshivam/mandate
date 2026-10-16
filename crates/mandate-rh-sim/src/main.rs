//! The runnable simulator for the founder's rehearsal (DEC-922 item 4, DEC-926):
//!
//! ```text
//! mandate-rh-sim --script <file> [--port <n>]
//! ```
//!
//! serves the script's core on `127.0.0.1` behind the OAuth login, and prints
//! `mandate-rh-sim: serving <endpoint>` once it listens. It takes no other argument, so nothing
//! can point it anywhere but loopback; a bad argument or script exits with status 2 before
//! anything listens.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use mandate_rh_sim::{Script, SimServer};

const USAGE: &str = "usage: mandate-rh-sim --script <file> [--port <n>]";

/// The arguments: the script's path, and the port, `0` for one the system picks.
struct Options {
    script: PathBuf,
    port: u16,
}

/// `--script` exactly once and `--port` at most once, each with its value; nothing else.
fn options(args: &[String]) -> Result<Options, String> {
    let (mut script, mut port) = (None, None);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let repeated = match flag.as_str() {
            "--script" => script.replace(PathBuf::from(value)).is_some(),
            "--port" => {
                let parsed = value
                    .parse::<u16>()
                    .map_err(|_| "--port takes 0 to 65535")?;
                port.replace(parsed).is_some()
            }
            other => return Err(format!("unknown argument {other}")),
        };
        if repeated {
            return Err(format!("{flag} is given twice"));
        }
    }
    let script = script.ok_or("--script is required")?;
    Ok(Options {
        script,
        port: port.unwrap_or(0),
    })
}

fn serve(args: &[String]) -> Result<SimServer, String> {
    let options = options(args)?;
    let text = std::fs::read_to_string(&options.script)
        .map_err(|error| format!("reading the script: {error}"))?;
    let script = Script::parse(&text).map_err(|error| format!("the script: {error}"))?;
    SimServer::rehearse(script, options.port).map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let server = match serve(&args) {
        Ok(server) => server,
        Err(why) => {
            drop(writeln!(
                std::io::stderr(),
                "mandate-rh-sim: {why}\n{USAGE}"
            ));
            return ExitCode::from(2);
        }
    };
    let announced = server.url().map_err(|e| e.to_string()).and_then(|url| {
        let mut out = std::io::stdout();
        writeln!(out, "mandate-rh-sim: serving {url}")
            .and_then(|()| out.flush())
            .map_err(|e| e.to_string())
    });
    let served = announced.and_then(|()| server.wait().map_err(|e| e.to_string()));
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            drop(writeln!(std::io::stderr(), "mandate-rh-sim: {why}"));
            ExitCode::FAILURE
        }
    }
}
