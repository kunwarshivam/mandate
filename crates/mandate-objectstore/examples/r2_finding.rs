//! The run against Cloudflare R2 that tests DEC-973 item 5's finding, from `r2-lockcheck.yml` on
//! `main` only, in the `production` environment (ES-12). It never touches the cell's buckets: it
//! creates a scratch bucket, runs the lock check against R2's S3 API, sets R2's strongest bucket
//! lock rule (indefinite, the whole bucket), shows that the rule refuses a delete, removes the rule
//! with the bucket-configuration token, shows that the same delete then succeeds, and deletes the
//! scratch bucket whatever happened. It prints outcomes and refusal codes only: never a key, a
//! token, or an account's identifier. Exits 0 when R2 behaves as DEC-973 found (the S3 check gives
//! no certificate, refused or failing closed); 1 when it does
//! not, which reopens DEC-973 rather than being acted on.
//!
//! It reads the `production` environment's names: `R2_ENDPOINT`, `R2_ACCESS_KEY_ID`,
//! `R2_SECRET_ACCESS_KEY`, `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`.

use std::env;
use std::process::ExitCode;

use jiff::Timestamp;
use mandate_objectstore::check::{CheckError, PROBE_KEY, check_s3};
use mandate_objectstore::r2::{CLOUDFLARE_API, R2LockApi, check_r2};
use mandate_objectstore::s3::{Endpoint, Outcome, S3};
use mandate_objectstore::sigv4::Credentials;
use secrecy::{ExposeSecret, SecretString};

const PREFIX: &str = "cold/";
const LOCKED_KEY: &str = "finding/locked";

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

fn shown(outcome: &Result<Outcome, impl std::fmt::Display>) -> String {
    match outcome {
        Ok(Outcome::Done) => "done".to_owned(),
        Ok(Outcome::Rejected(rejected)) => {
            format!("rejected ({} {})", rejected.status, rejected.code)
        }
        Err(error) => format!("no answer ({error})"),
    }
}

/// Cloudflare's bucket API for the scratch bucket, with the bucket-configuration token.
struct Buckets {
    http: reqwest::Client,
    account: String,
    token: SecretString,
}

impl Buckets {
    fn url(&self, tail: &str) -> String {
        format!(
            "{CLOUDFLARE_API}/accounts/{}/r2/buckets{tail}",
            self.account
        )
    }

    async fn call(
        &self,
        method: reqwest::Method,
        tail: &str,
        body: Option<String>,
    ) -> Result<(), String> {
        let mut request = self
            .http
            .request(method, self.url(tail))
            .bearer_auth(self.token.expose_secret());
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body);
        }
        let status = request
            .send()
            .await
            .map_err(|_| "Cloudflare's API could not be reached".to_owned())?
            .status();
        if status.is_success() {
            Ok(())
        } else {
            Err(format!("Cloudflare's API answered {status}"))
        }
    }

    async fn set_rules(&self, bucket: &str, rules: &str) -> Result<(), String> {
        self.call(
            reqwest::Method::PUT,
            &format!("/{bucket}/lock"),
            Some(format!("{{\"rules\":{rules}}}")),
        )
        .await
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(true) => {
            println!("R2 behaves as DEC-973 item 5 found: its lock is governance-equivalent");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!(
                "R2 does not behave as DEC-973 item 5 found: reopen DEC-973, do not act on this"
            );
            ExitCode::FAILURE
        }
        Err(problem) => {
            eprintln!("r2_finding: {problem}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<bool, String> {
    let bucket = format!("mandate-lockcheck-{}", Timestamp::now().as_second());
    let endpoint =
        Endpoint::new(&setting("R2_ENDPOINT")?, "auto", &bucket).map_err(|e| e.to_string())?;
    let key = Credentials::new(
        setting("R2_ACCESS_KEY_ID")?,
        SecretString::from(setting("R2_SECRET_ACCESS_KEY")?),
        None,
    );
    let store = S3::new(endpoint, key, Timestamp::now).map_err(|e| e.to_string())?;
    let buckets = Buckets {
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "the HTTP client could not be built".to_owned())?,
        account: setting("CLOUDFLARE_ACCOUNT_ID")?,
        token: SecretString::from(setting("CLOUDFLARE_API_TOKEN")?),
    };
    buckets
        .call(
            reqwest::Method::POST,
            "",
            Some(format!(
                "{{\"name\":\"{bucket}\",\"locationHint\":\"enam\"}}"
            )),
        )
        .await?;
    println!("created the scratch bucket {bucket}");
    let confirmed = probe(&store, &buckets, &bucket).await;
    let teardown = teardown(&store, &buckets, &bucket).await;
    println!("teardown: {}", teardown.as_deref().unwrap_or("done"));
    confirmed
}

async fn probe(store: &S3, buckets: &Buckets, bucket: &str) -> Result<bool, String> {
    let unruled = check_s3(store, PREFIX).await.map(|_| ());
    println!("1. S3 Object Lock check, no rule: {}", verdict(&unruled));

    buckets
        .set_rules(
            bucket,
            r#"[{"id":"mandate-lockcheck","enabled":true,"condition":{"type":"Indefinite"}}]"#,
        )
        .await?;
    println!("2. set R2's strongest rule: indefinite, the whole bucket");
    let api = R2LockApi::new(
        CLOUDFLARE_API,
        &buckets.account,
        bucket,
        buckets.token.clone(),
    )
    .map_err(|e| e.to_string())?;
    let rules = check_r2(&api, PREFIX).await;
    println!("3. R2 bucket lock rules: {}", verdict(&rules));
    let ruled = check_s3(store, PREFIX).await.map(|_| ());
    println!(
        "4. S3 Object Lock check, with the rule: {}",
        verdict(&ruled)
    );

    let written = store
        .put_object(LOCKED_KEY, b"locked by the bucket rule\n", None)
        .await;
    println!(
        "5. write an object under the rule: {}",
        match &written {
            Ok(Ok(_)) => "done".to_owned(),
            Ok(Err(rejected)) => format!("rejected ({} {})", rejected.status, rejected.code),
            Err(error) => format!("no answer ({error})"),
        }
    );
    let held = store.delete_object(LOCKED_KEY).await;
    println!("6. delete it while the rule holds: {}", shown(&held));

    buckets.set_rules(bucket, "[]").await?;
    println!("7. remove the rule with the bucket-configuration token: done");
    let released = store.delete_object(LOCKED_KEY).await;
    println!("8. delete it again: {}", shown(&released));
    let gone = store.get_object(LOCKED_KEY, None).await;
    println!("9. the object is gone: {}", matches!(gone, Ok(None)));

    Ok(unruled.is_err()
        && matches!(&rules, Err(CheckError::Refused(_)))
        && ruled.is_err()
        && matches!(written, Ok(Ok(_)))
        && matches!(held, Ok(Outcome::Rejected(_)))
        && matches!(released, Ok(Outcome::Done))
        && matches!(gone, Ok(None)))
}

/// Removes any rule, the objects the run may have left, and the bucket; the first problem, if any.
async fn teardown(store: &S3, buckets: &Buckets, bucket: &str) -> Option<String> {
    let mut problems = Vec::new();
    if let Err(problem) = buckets.set_rules(bucket, "[]").await {
        problems.push(problem);
    }
    for key in [LOCKED_KEY.to_owned(), format!("{PREFIX}{PROBE_KEY}")] {
        if let Err(error) = store.delete_object(&key).await {
            problems.push(error.to_string());
        }
    }
    if let Err(problem) = buckets
        .call(reqwest::Method::DELETE, &format!("/{bucket}"), None)
        .await
    {
        problems.push(format!(
            "{problem}; delete the scratch bucket {bucket} by hand"
        ));
    }
    problems.into_iter().next()
}
