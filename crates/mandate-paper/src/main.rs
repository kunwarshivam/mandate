//! `mandate-paper`: one run of the production cycle on the founder's Alpaca **paper** account
//! (E7-19 slice 5, the first paper trade brief's E1a, DEC-846). All of it is
//! [`mandate_paper::process`]; this prints its lines, or its error's message on stderr and exits
//! non-zero.

use std::process::ExitCode;

fn main() -> ExitCode {
    match mandate_paper::process(std::env::args().skip(1), std::env::vars()) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
