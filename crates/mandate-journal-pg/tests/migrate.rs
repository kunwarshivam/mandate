//! The migration runner behind `mandate db migrate` (backlog E21-50, ADR-0001 ES-08, OPS-13,
//! DEC-994): forward only, in order, each file once under the owner role, in one transaction that
//! a refusal or a failing file rolls back whole; a downgrade, a changed file, a ledger out of
//! order and an unrecorded schema are refused. The ledger check is a pure function, fuzzed here
//! against an oracle of its own; the rest runs against `MANDATE_PG_URL`, with migrations of the
//! tests' own so forward steps, a failure and a downgrade exist before `migrations/` has them.
#![allow(
    clippy::disallowed_methods,
    reason = "these tests drive the migration command's runner directly"
)]

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use mandate_canon::Digest;
use mandate_journal_pg::{
    APP_ROLE, MigrateError, Migration, OWNER_ROLE, Recorded, applied_prefix, migrate, migrate_known,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool, raw_sql};
use tokio::runtime::Runtime;

const URL_VAR: &str = "MANDATE_PG_URL";
const REQUIRED_VAR: &str = "MANDATE_PG_REQUIRED";
const DIVISION_BY_ZERO: &str = "22012";

const ONE: Migration = Migration {
    version: 1,
    file: "0001_one.sql",
    sql: "CREATE TABLE one (x int);",
};
const TWO: Migration = Migration {
    version: 2,
    file: "0002_two.sql",
    sql: "CREATE TABLE two (x int);\nCREATE FUNCTION two_fn() RETURNS int LANGUAGE sql AS 'SELECT 2';",
};
const THREE: Migration = Migration {
    version: 3,
    file: "0003_three.sql",
    sql: "CREATE TABLE three (x int);",
};
const FAILS: Migration = Migration {
    version: 2,
    file: "0002_fails.sql",
    sql: "CREATE TABLE fails (x int);\nSELECT 1 / 0;",
};

fn row(version: i64, file: &str, sha256: &str) -> Recorded {
    Recorded {
        version,
        file: file.to_owned(),
        sha256: sha256.to_owned(),
    }
}

/// What `applied_prefix` must answer, computed another way: count each version, and compare the
/// counted set with `1..=n` as a whole before looking at any row's content.
fn oracle(known: &[Recorded], applied: &[Recorded]) -> Result<usize, &'static str> {
    let beyond = i64::try_from(known.len()).unwrap();
    if applied.iter().any(|r| r.version > beyond) {
        return Err("schema_newer");
    }
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for r in applied {
        *counts.entry(r.version).or_default() += 1;
    }
    let expected: BTreeMap<i64, usize> = (1..=i64::try_from(applied.len()).unwrap())
        .map(|v| (v, 1))
        .collect();
    if counts != expected {
        return Err("migration_out_of_order");
    }
    let mismatched = applied.iter().any(|r| {
        let k = &known[usize::try_from(r.version - 1).unwrap()];
        k.file != r.file || k.sha256 != r.sha256
    });
    if mismatched {
        return Err("migration_changed");
    }
    Ok(applied.len())
}

#[derive(Debug, Clone)]
enum Flaw {
    None,
    Drop(usize),
    Repeat(usize),
    Content(usize),
    Name(usize),
    Zero,
    Newer(i64),
}

fn flaw() -> impl Strategy<Value = Flaw> {
    prop_oneof![
        3 => Just(Flaw::None),
        1 => (0usize..8).prop_map(Flaw::Drop),
        1 => (0usize..8).prop_map(Flaw::Repeat),
        1 => (0usize..8).prop_map(Flaw::Content),
        1 => (0usize..8).prop_map(Flaw::Name),
        1 => Just(Flaw::Zero),
        1 => (1i64..4).prop_map(Flaw::Newer),
    ]
}

/// Every ledger the runner can meet, honest or not, gets the oracle's answer, and an honest one is
/// read as exactly its length.
#[test]
fn the_ledger_check_matches_an_independent_oracle() {
    let strategy = (
        1usize..6,
        0usize..6,
        flaw(),
        any::<u64>(),
        proptest::collection::vec(0usize..6, 0..6),
    );
    let mut runner = TestRunner::new(Config {
        cases: 2048,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&strategy, |(k, prefix, flaw, seed, swaps)| {
        let known: Vec<Recorded> = (1..=k)
            .map(|v| {
                let file = format!("{v:04}_m{seed}.sql");
                let sha = Digest::of(file.as_bytes()).to_hex();
                row(i64::try_from(v).unwrap(), &file, &sha)
            })
            .collect();
        let mut applied: Vec<Recorded> = known[..prefix.min(k)].to_vec();
        let at = |i: usize, len: usize| (len > 0).then(|| i % len);
        match flaw {
            Flaw::None => {}
            Flaw::Drop(i) => {
                if let Some(i) = at(i, applied.len()) {
                    applied.remove(i);
                }
            }
            Flaw::Repeat(i) => {
                if let Some(i) = at(i, applied.len()) {
                    applied.push(applied[i].clone());
                }
            }
            Flaw::Content(i) => {
                if let Some(i) = at(i, applied.len()) {
                    applied[i].sha256 = Digest::of(b"edited").to_hex();
                }
            }
            Flaw::Name(i) => {
                if let Some(i) = at(i, applied.len()) {
                    applied[i].file = format!("{}_renamed.sql", applied[i].version);
                }
            }
            Flaw::Zero => applied.push(row(0, "0000_zero.sql", &"0".repeat(64))),
            Flaw::Newer(by) => {
                let version = i64::try_from(k).unwrap() + by;
                applied.push(row(version, "9999_newer.sql", &"0".repeat(64)));
            }
        }
        for s in swaps {
            if let Some(i) = at(s, applied.len()) {
                let last = applied.len() - 1;
                applied.swap(i, last);
            }
        }
        let got = applied_prefix(&known, &applied).map_err(|e| e.code());
        prop_assert_eq!(got, oracle(&known, &applied), "{:?}", applied);
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

#[test]
fn the_ledger_check_names_what_it_refuses() {
    let known: Vec<Recorded> = [ONE, TWO, THREE].iter().map(Migration::recorded).collect();
    assert_eq!(applied_prefix(&known, &[]).ok(), Some(0));
    assert_eq!(applied_prefix(&known, &known).ok(), Some(3));
    let reversed: Vec<Recorded> = known.iter().rev().cloned().collect();
    assert_eq!(
        applied_prefix(&known, &reversed).ok(),
        Some(3),
        "rows are read by version, not by the order they arrive in"
    );
    let newest = [
        known[0].clone(),
        row(7, "0007_x.sql", &known[0].sha256),
        row(5, "0005_y.sql", &known[0].sha256),
    ];
    assert!(
        matches!(
            applied_prefix(&known, &newest),
            Err(MigrateError::SchemaNewer { version: 7 })
        ),
        "the newest unknown version is the one named"
    );
    let gap = [known[0].clone(), known[2].clone()];
    let refused = applied_prefix(&known, &gap);
    assert!(
        matches!(&refused, Err(MigrateError::OutOfOrder { version: 3, file }) if file == THREE.file),
        "{refused:?}"
    );
    let edited = [known[0].clone(), row(2, TWO.file, &known[0].sha256)];
    let refused = applied_prefix(&known, &edited);
    assert!(
        matches!(&refused, Err(MigrateError::Changed { file }) if file == TWO.file),
        "{refused:?}"
    );
}

/// A refusal's message names the file or version it is about, and its code; never more.
#[test]
fn every_refusal_has_a_stable_code_and_a_plain_message() {
    let cases: [(MigrateError, &str, &str); 8] = [
        (MigrateError::SchemaInvalid, "schema_invalid", "schema name"),
        (MigrateError::PrivilegeMissing, "privilege_missing", "42501"),
        (
            MigrateError::OwnerRoleNotHeld,
            "owner_role_not_held",
            OWNER_ROLE,
        ),
        (MigrateError::SchemaNotOwned, "schema_not_owned", OWNER_ROLE),
        (
            MigrateError::SchemaUnrecorded,
            "schema_unrecorded",
            "no migration ledger",
        ),
        (
            MigrateError::SchemaNewer { version: 12 },
            "schema_newer",
            "version 12",
        ),
        (
            MigrateError::Changed {
                file: "0001_a.sql".to_owned(),
            },
            "migration_changed",
            "0001_a.sql",
        ),
        (
            MigrateError::Failed {
                file: "0002_b.sql",
                sqlstate: DIVISION_BY_ZERO.to_owned(),
            },
            "migration_failed",
            "0002_b.sql failed with SQLSTATE 22012",
        ),
    ];
    for (error, code, shown) in cases {
        assert_eq!(error.code(), code);
        let message = error.to_string();
        assert!(message.contains(shown), "{code}: {message}");
    }
    let out_of_order = MigrateError::OutOfOrder {
        version: 3,
        file: "0003_c.sql".to_owned(),
    };
    assert_eq!(out_of_order.code(), "migration_out_of_order");
    assert!(out_of_order.to_string().contains("version 3 (0003_c.sql)"));
    let down = MigrateError::Unavailable(sqlx::Error::PoolTimedOut);
    assert_eq!(down.code(), "unavailable");
}

static SCHEMAS: AtomicU64 = AtomicU64::new(0);

/// A schema name of this test's own in the database `MANDATE_PG_URL` names, with a superuser pool;
/// the schema is dropped when the value is.
struct Scratch {
    rt: Runtime,
    base: PgConnectOptions,
    admin: PgPool,
    schema: String,
}

impl Scratch {
    fn new() -> Option<Self> {
        let Ok(url) = std::env::var(URL_VAR) else {
            assert!(
                std::env::var_os(REQUIRED_VAR).is_none(),
                "{REQUIRED_VAR} is set but {URL_VAR} is not"
            );
            eprintln!("skipping: {URL_VAR} is unset");
            return None;
        };
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let schema = format!(
            "k0_{}_{}",
            std::process::id(),
            SCHEMAS.fetch_add(1, Ordering::Relaxed)
        );
        let base = PgConnectOptions::from_str(&url).unwrap();
        let admin = {
            let _entered = rt.enter();
            PgPoolOptions::new()
                .max_connections(4)
                .connect_lazy_with(base.clone())
        };
        let scratch = Self {
            rt,
            base,
            admin,
            schema,
        };
        scratch.sql(&format!("DROP SCHEMA IF EXISTS {} CASCADE", scratch.schema));
        Some(scratch)
    }

    fn sql(&self, sql: &str) {
        self.rt
            .block_on(raw_sql(AssertSqlSafe(sql.to_owned())).execute(&self.admin))
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    fn run(
        &self,
        known: &[Migration],
    ) -> Result<mandate_journal_pg::MigrationReport, MigrateError> {
        self.rt
            .block_on(migrate_known(&self.admin, &self.schema, known))
    }

    fn ledger(&self) -> Vec<Recorded> {
        let rows: Vec<(i64, String, String)> = self
            .rt
            .block_on(
                sqlx::query_as(AssertSqlSafe(format!(
                    "SELECT version, file, sha256 FROM {}.schema_migrations ORDER BY version",
                    self.schema
                )))
                .fetch_all(&self.admin),
            )
            .unwrap();
        rows.into_iter().map(|(v, f, s)| row(v, &f, &s)).collect()
    }

    /// The schema's tables and functions with their owners, sorted.
    fn objects(&self) -> Vec<(String, String)> {
        self.rt
            .block_on(
                sqlx::query_as(
                    "SELECT c.relname::text, pg_get_userbyid(c.relowner)::text FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relkind = 'r' \
                     UNION ALL \
                     SELECT p.proname::text, pg_get_userbyid(p.proowner)::text FROM pg_proc p \
                     JOIN pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = $1 \
                     ORDER BY 1",
                )
                .bind(&self.schema)
                .fetch_all(&self.admin),
            )
            .unwrap()
    }

    fn schema_exists(&self) -> bool {
        self.rt
            .block_on(
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)")
                    .bind(&self.schema)
                    .fetch_one(&self.admin),
            )
            .unwrap()
    }

    fn privilege(&self, query: &str) -> bool {
        self.rt
            .block_on(sqlx::query_scalar(AssertSqlSafe(query.to_owned())).fetch_one(&self.admin))
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let drop_it = format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema);
        self.rt
            .block_on(raw_sql(AssertSqlSafe(drop_it)).execute(&self.admin))
            .ok();
    }
}

fn owned(names: &[&str]) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = names
        .iter()
        .map(|n| ((*n).to_owned(), OWNER_ROLE.to_owned()))
        .collect();
    all.sort();
    all
}

#[test]
fn migrations_apply_forward_once_each_owned_by_the_owner_role() {
    let Some(db) = Scratch::new() else { return };
    let first = db.run(&[ONE]).unwrap();
    assert_eq!(
        (
            first.took_over,
            first.already.clone(),
            first.applied.clone(),
            first.version
        ),
        (0, vec![], vec![ONE.file], 1)
    );
    let second = db.run(&[ONE, TWO]).unwrap();
    assert_eq!(
        (
            second.already.clone(),
            second.applied.clone(),
            second.version
        ),
        (vec![ONE.file], vec![TWO.file], 2)
    );
    let third = db.run(&[ONE, TWO]).unwrap();
    assert_eq!(
        (third.already.clone(), third.applied.clone(), third.version),
        (vec![ONE.file, TWO.file], vec![], 2),
        "a run with nothing to do changes nothing"
    );
    assert_eq!(db.ledger(), vec![ONE.recorded(), TWO.recorded()]);
    assert_eq!(
        db.objects(),
        owned(&["one", "schema_migrations", "two", "two_fn"]),
        "every object, the ledger too, belongs to {OWNER_ROLE}"
    );
    let s = &db.schema;
    assert!(
        db.privilege(&format!(
            "SELECT has_schema_privilege('{APP_ROLE}', '{s}', 'USAGE')"
        )),
        "the application role may use the schema"
    );
    assert!(
        !db.privilege(&format!(
            "SELECT has_table_privilege('{APP_ROLE}', '{s}.schema_migrations', 'SELECT')"
        )),
        "and may not read the ledger"
    );
    assert!(
        !db.privilege(&format!(
            "SELECT has_schema_privilege('{APP_ROLE}', '{s}', 'CREATE')"
        )),
        "nor create objects in the schema"
    );
}

#[test]
fn a_downgrade_is_refused_and_changes_nothing() {
    let Some(db) = Scratch::new() else { return };
    db.run(&[ONE, TWO]).unwrap();
    let refused = db.run(&[ONE]).err();
    assert!(
        matches!(refused, Some(MigrateError::SchemaNewer { version: 2 })),
        "{refused:?}"
    );
    let empty = db.run(&[]).err();
    assert!(
        matches!(empty, Some(MigrateError::SchemaNewer { version: 2 })),
        "{empty:?}"
    );
    assert_eq!(db.ledger(), vec![ONE.recorded(), TWO.recorded()]);
}

#[test]
fn a_failing_file_rolls_the_whole_run_back() {
    let Some(db) = Scratch::new() else { return };
    let refused = db.run(&[ONE, FAILS]).err();
    assert!(
        matches!(&refused, Some(MigrateError::Failed { file, sqlstate })
            if *file == FAILS.file && sqlstate == DIVISION_BY_ZERO),
        "{refused:?}"
    );
    assert!(
        !db.schema_exists(),
        "neither the schema, its ledger, nor the first file's table survives"
    );
    db.run(&[ONE]).unwrap();
    let refused = db.run(&[ONE, FAILS]).err().map(|e| e.code());
    assert_eq!(refused, Some("migration_failed"));
    assert_eq!(db.ledger(), vec![ONE.recorded()]);
    assert_eq!(db.objects(), owned(&["one", "schema_migrations"]));
}

#[test]
fn a_changed_or_out_of_order_ledger_is_refused() {
    let Some(db) = Scratch::new() else { return };
    db.run(&[ONE]).unwrap();
    let edited = Migration {
        sql: "CREATE TABLE one (x bigint);",
        ..ONE
    };
    let refused = db.run(&[edited, TWO]).err();
    assert!(
        matches!(&refused, Some(MigrateError::Changed { file }) if file == ONE.file),
        "{refused:?}"
    );
    let s = &db.schema;
    db.sql(&format!(
        "INSERT INTO {s}.schema_migrations (version, file, sha256, source) \
         VALUES (3, '{}', '{}', 'planted')",
        THREE.file,
        THREE.sha256()
    ));
    let refused = db.run(&[ONE, TWO, THREE]).err();
    assert!(
        matches!(&refused, Some(MigrateError::OutOfOrder { version: 3, .. })),
        "{refused:?}"
    );
    assert_eq!(db.objects(), owned(&["one", "schema_migrations"]));
}

#[test]
fn a_schema_it_does_not_know_is_refused() {
    let Some(db) = Scratch::new() else { return };
    let s = db.schema.clone();
    db.sql(&format!(
        "CREATE SCHEMA {s} AUTHORIZATION {OWNER_ROLE}; CREATE TABLE {s}.stray (x int); \
         ALTER TABLE {s}.stray OWNER TO {OWNER_ROLE}"
    ));
    let refused = db.run(&[ONE]).err().map(|e| e.code());
    assert_eq!(refused, Some("schema_unrecorded"));
    assert_eq!(db.objects(), owned(&["stray"]), "nothing was added");

    db.sql(&format!(
        "DROP TABLE {s}.stray; CREATE FUNCTION {s}.stray_fn() RETURNS int LANGUAGE sql AS 'SELECT 1'; \
         ALTER FUNCTION {s}.stray_fn() OWNER TO {OWNER_ROLE}"
    ));
    let refused = db.run(&[ONE]).err().map(|e| e.code());
    assert_eq!(refused, Some("schema_unrecorded"), "a function counts too");

    db.sql(&format!(
        "DROP SCHEMA {s} CASCADE; CREATE SCHEMA {s} AUTHORIZATION CURRENT_USER"
    ));
    let refused = db.run(&[ONE]).err().map(|e| e.code());
    assert_eq!(refused, Some("schema_not_owned"));
    assert_eq!(db.objects(), vec![]);

    db.sql(&format!(
        "DROP SCHEMA {s}; CREATE SCHEMA {s} AUTHORIZATION {OWNER_ROLE}"
    ));
    let empty = db.run(&[ONE]).unwrap();
    assert_eq!(
        empty.applied,
        vec![ONE.file],
        "an empty schema it owns is fresh"
    );
}

#[test]
fn a_bad_schema_name_is_refused_before_connecting() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pool = {
        let _entered = rt.enter();
        PgPoolOptions::new()
            .connect_lazy("postgres://k0-host-sentinel.invalid:1/db")
            .unwrap()
    };
    let long = "a".repeat(64);
    for schema in [
        "",
        "Journal",
        "a-b",
        "1abc",
        "a;drop",
        "a b",
        "é",
        "a\"b",
        long.as_str(),
    ] {
        let refused = rt.block_on(migrate(&pool, schema)).err();
        assert!(
            matches!(refused, Some(MigrateError::SchemaInvalid)),
            "{schema:?}: {refused:?}"
        );
    }
    let longest = "a".repeat(63);
    for schema in ["journal", "_x", "e5_3_1", "z9", longest.as_str()] {
        let refused = rt.block_on(migrate(&pool, schema)).err();
        assert!(
            matches!(refused, Some(MigrateError::Unavailable(_))),
            "{schema:?} is a valid name, so the run goes on to connect: {refused:?}"
        );
    }
}

#[test]
fn concurrent_runs_apply_each_file_once() {
    let Some(db) = Scratch::new() else { return };
    let outcomes = db.rt.block_on(async {
        let runs = (0..4).map(|_| {
            let (pool, schema) = (db.admin.clone(), db.schema.clone());
            tokio::spawn(async move { migrate_known(&pool, &schema, &[ONE, TWO]).await })
        });
        let mut outcomes = Vec::new();
        for run in runs.collect::<Vec<_>>() {
            outcomes.push(run.await.unwrap().unwrap());
        }
        outcomes
    });
    let applied: Vec<&str> = outcomes.iter().flat_map(|o| o.applied.clone()).collect();
    assert_eq!(applied, vec![ONE.file, TWO.file], "{outcomes:?}");
    assert!(outcomes.iter().all(|o| o.version == 2));
    assert_eq!(db.ledger(), vec![ONE.recorded(), TWO.recorded()]);
}

#[test]
fn a_role_that_cannot_act_as_the_owner_is_refused() {
    let Some(db) = Scratch::new() else { return };
    db.sql(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'k0_outsider') THEN \
             CREATE ROLE k0_outsider LOGIN PASSWORD 'k0-outsider'; \
           END IF; \
         EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; \
         END $$",
    );
    db.run(&[]).unwrap();
    let outsider = {
        let _entered = db.rt.enter();
        PgPoolOptions::new().max_connections(1).connect_lazy_with(
            db.base
                .clone()
                .username("k0_outsider")
                .password("k0-outsider"),
        )
    };
    let refused = db
        .rt
        .block_on(migrate_known(&outsider, &db.schema, &[ONE]))
        .err();
    assert!(
        matches!(refused, Some(MigrateError::OwnerRoleNotHeld)),
        "{refused:?}"
    );
    assert_eq!(
        db.objects(),
        owned(&["schema_migrations"]),
        "nothing was applied"
    );
}

/// A role that may act as the owner but not create the schema is refused `privilege_missing`,
/// not reported as an outage.
#[test]
fn a_role_without_a_needed_privilege_is_refused_as_such() {
    let Some(db) = Scratch::new() else { return };
    db.run(&[]).unwrap();
    db.sql(&format!(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'k0_member') THEN \
             CREATE ROLE k0_member LOGIN PASSWORD 'k0-member'; \
           END IF; \
         EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; \
         END $$; \
         GRANT {OWNER_ROLE} TO k0_member; DROP SCHEMA {} CASCADE",
        db.schema
    ));
    let member = {
        let _entered = db.rt.enter();
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(db.base.clone().username("k0_member").password("k0-member"))
    };
    let refused = db
        .rt
        .block_on(migrate_known(&member, &db.schema, &[ONE]))
        .err();
    assert!(
        matches!(refused, Some(MigrateError::PrivilegeMissing)),
        "{refused:?}"
    );
    assert!(!db.schema_exists(), "nothing was created");
}
