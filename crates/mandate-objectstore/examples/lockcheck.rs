//! The lock check against any S3-compatible provider, from the command line: Backblaze B2 if
//! DEC-974 is taken, or any other candidate (R2's own run is `r2_finding`, DEC-973 item 5). It runs [`check_s3`] with the writer's key, and with an R2 account also
//! [`check_r2`] over the bucket's lock rules, and prints the verdicts as refusal codes only: never
//! a key, a token, or an object's content. Exits 0 only when the S3 check passes.
//!
//! Configuration (`crates/mandate-objectstore/README.md`): `MANDATE_OBJECTSTORE_URL`,
//! `MANDATE_OBJECTSTORE_REGION`, `MANDATE_OBJECTSTORE_BUCKET`, `MANDATE_OBJECTSTORE_PREFIX`, and
//! the files holding the writer's key, `MANDATE_OBJECTSTORE_KEY_ID_FILE` and
//! `MANDATE_OBJECTSTORE_SECRET_FILE`; for R2's rules, `MANDATE_R2_ACCOUNT_ID` and
//! `MANDATE_R2_TOKEN_FILE`, a token that can read the bucket's configuration and nothing more.

use std::env;
use std::process::ExitCode;

use jiff::Timestamp;
use mandate_objectstore::check::{CheckError, check_s3};
use mandate_objectstore::r2::{CLOUDFLARE_API, R2LockApi, check_r2};
use mandate_objectstore::s3::{Endpoint, S3};
use mandate_objectstore::sigv4::Credentials;

fn setting(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}

fn verdict(result: &Result<(), CheckError>) -> String {
    match result {
        Ok(()) => "accepted".to_owned(),
        Err(CheckError::Refused(refused)) => format!("refused: {refused}"),
        Err(CheckError::Store(error)) => format!("no verdict, fails closed: {error}"),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(problem) => {
            eprintln!("lockcheck: {problem}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<bool, String> {
    let endpoint = Endpoint::new(
        &setting("MANDATE_OBJECTSTORE_URL")?,
        &setting("MANDATE_OBJECTSTORE_REGION")?,
        &setting("MANDATE_OBJECTSTORE_BUCKET")?,
    )
    .map_err(|error| error.to_string())?;
    let prefix = setting("MANDATE_OBJECTSTORE_PREFIX")?;
    let key = Credentials::from_files(
        setting("MANDATE_OBJECTSTORE_KEY_ID_FILE")?.as_ref(),
        setting("MANDATE_OBJECTSTORE_SECRET_FILE")?.as_ref(),
    )
    .map_err(|_| "the writer's key files could not be read".to_owned())?;
    let store = S3::new(endpoint, key, Timestamp::now).map_err(|error| error.to_string())?;
    let s3 = check_s3(&store, &prefix).await.map(|_| ());
    println!("S3 Object Lock check: {}", verdict(&s3));
    if let Ok(account) = env::var("MANDATE_R2_ACCOUNT_ID") {
        let token = std::fs::read_to_string(setting("MANDATE_R2_TOKEN_FILE")?)
            .map_err(|_| "the R2 token file could not be read".to_owned())?;
        let api = R2LockApi::new(
            CLOUDFLARE_API,
            &account,
            store.endpoint().bucket(),
            token.trim().to_owned().into(),
        )
        .map_err(|error| error.to_string())?;
        println!(
            "R2 bucket lock rules: {}",
            verdict(&check_r2(&api, &prefix).await)
        );
    }
    Ok(s3.is_ok())
}
