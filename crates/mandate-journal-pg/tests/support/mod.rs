//! Test databases for the Postgres journal. Each test gets its own schema in the database named by
//! `MANDATE_PG_URL`, migrated as the owner role, and pools that act as the owner, the application
//! role, and the connecting superuser (for planting faults and tampering). Without
//! `MANDATE_PG_URL` a test says so and passes, unless `MANDATE_PG_REQUIRED` is set (CI's `full`
//! job), where a missing database fails the test instead of skipping it (DEC-109).
#![allow(
    dead_code,
    reason = "each test crate that includes this module uses a different subset"
)]

use std::future::Future;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use mandate_journal::{AppendOutcome, Head, StoredEvent, StreamId};
use mandate_journal_pg::{APP_ROLE, OWNER_ROLE, PgJournal, migrator};
use mandate_time::UtcNanos;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, Executor, PgPool, raw_sql};
use tokio::runtime::Runtime;

use super::conformance::Backend;

pub const URL_VAR: &str = "MANDATE_PG_URL";
pub const REQUIRED_VAR: &str = "MANDATE_PG_REQUIRED";

/// Serializes role creation across concurrent test processes (roles are cluster-wide).
const ROLE_LOCK: i64 = 0x4535_3350_4752;

static SCHEMAS: AtomicU64 = AtomicU64::new(0);

/// A migrated schema of its own, with pools for each role, dropped when the value is.
pub struct TestDb {
    pub rt: Runtime,
    pub schema: String,
    pub owner: PgPool,
    pub app: PgPool,
    pub admin: PgPool,
    base: PgConnectOptions,
}

impl TestDb {
    /// `None` (after saying why) when `MANDATE_PG_URL` is unset and not required. The crate must
    /// embed its schema either way, so a journal without migrations fails even where no database
    /// is available.
    pub fn new() -> Option<Self> {
        assert!(
            migrator().iter().next().is_some(),
            "mandate-journal-pg embeds no migrations"
        );
        let Ok(url) = std::env::var(URL_VAR) else {
            assert!(
                std::env::var_os(REQUIRED_VAR).is_none(),
                "{REQUIRED_VAR} is set but {URL_VAR} is not: the Postgres journal tests must run here"
            );
            eprintln!(
                "skipping: {URL_VAR} is unset, so the Postgres journal tests do not run \
                 (see crates/mandate-journal-pg/README.md)"
            );
            return None;
        };
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let schema = format!(
            "e5_3_{}_{}",
            std::process::id(),
            SCHEMAS.fetch_add(1, Ordering::Relaxed)
        );
        let base = PgConnectOptions::from_str(&url)
            .unwrap()
            .application_name(&schema);
        let (owner, app, admin) = rt.block_on(async {
            let setup = PgPoolOptions::new()
                .max_connections(1)
                .connect_with(base.clone())
                .await
                .unwrap_or_else(|e| panic!("cannot connect to {URL_VAR}: {e}"));
            let mut tx = setup.begin().await.unwrap();
            sqlx::query("SELECT pg_advisory_xact_lock($1)")
                .bind(ROLE_LOCK)
                .execute(&mut *tx)
                .await
                .unwrap();
            for role in [OWNER_ROLE, APP_ROLE] {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
                        .bind(role)
                        .fetch_one(&mut *tx)
                        .await
                        .unwrap();
                if !exists {
                    raw_sql(AssertSqlSafe(format!("CREATE ROLE {role} NOLOGIN")))
                        .execute(&mut *tx)
                        .await
                        .unwrap();
                }
            }
            tx.commit().await.unwrap();
            raw_sql(AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE;
                 CREATE SCHEMA {schema} AUTHORIZATION {OWNER_ROLE};
                 GRANT USAGE ON SCHEMA {schema} TO {APP_ROLE}"
            )))
            .execute(&setup)
            .await
            .unwrap();
            setup.close().await;
            let owner = pool(&base, &schema, Some(OWNER_ROLE), 2);
            let app = pool(&base, &schema, Some(APP_ROLE), 8);
            let admin = pool(&base, &schema, None, 2);
            migrator().run(&owner).await.unwrap();
            (owner, app, admin)
        });
        Some(TestDb {
            rt,
            schema,
            owner,
            app,
            admin,
            base,
        })
    }

    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.rt.block_on(future)
    }

    pub fn journal(&self) -> PgJournal {
        PgJournal::new(self.app.clone())
    }

    /// A fresh pool acting as `role` (`None`: the connecting superuser) in this test's schema.
    pub fn pool_as(&self, role: Option<&str>) -> PgPool {
        let _runtime = self.rt.enter();
        pool(&self.base, &self.schema, role, 2)
    }

    /// Runs `sql` as the superuser in this test's schema.
    pub fn admin_execute(&self, sql: &str) {
        self.block_on(raw_sql(AssertSqlSafe(sql.to_owned())).execute(&self.admin))
            .unwrap();
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let schema = self.schema.clone();
        let base = self.base.clone();
        let (owner, app, admin) = (self.owner.clone(), self.app.clone(), self.admin.clone());
        self.rt.block_on(async move {
            owner.close().await;
            app.close().await;
            admin.close().await;
            if let Ok(pool) = PgPoolOptions::new()
                .max_connections(1)
                .connect_with(base)
                .await
            {
                raw_sql(AssertSqlSafe(format!(
                    "DROP SCHEMA IF EXISTS {schema} CASCADE"
                )))
                .execute(&pool)
                .await
                .ok();
                pool.close().await;
            }
        });
    }
}

/// A lazily connecting pool whose sessions use `schema` and, when given, act as `role`.
fn pool(base: &PgConnectOptions, schema: &str, role: Option<&str>, size: u32) -> PgPool {
    let setup = match role {
        Some(role) => format!("SET search_path TO {schema}; SET ROLE {role}"),
        None => format!("SET search_path TO {schema}"),
    };
    PgPoolOptions::new()
        .max_connections(size)
        .after_connect(move |conn, _| {
            let setup = setup.clone();
            Box::pin(async move {
                conn.execute(raw_sql(AssertSqlSafe(setup))).await?;
                Ok(())
            })
        })
        .connect_lazy_with(base.clone())
}

/// The Postgres journal as the conformance suite drives it.
pub struct PgBackend {
    pub db: TestDb,
    pub journal: PgJournal,
}

impl PgBackend {
    pub fn fresh() -> Option<Self> {
        let db = TestDb::new()?;
        let journal = db.journal();
        Some(Self { db, journal })
    }
}

impl Backend for PgBackend {
    fn take_ownership(&mut self, stream: &StreamId) -> u64 {
        self.db
            .block_on(self.journal.take_ownership(stream))
            .unwrap()
    }

    fn head(&mut self, stream: &StreamId) -> Head {
        self.db.block_on(self.journal.head(stream)).unwrap()
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        self.db
            .block_on(
                self.journal
                    .append(stream, expected_head, writer_epoch, recorded_at, drafts),
            )
            .unwrap()
    }

    fn rows(&mut self, stream: &StreamId) -> Vec<StoredEvent> {
        self.db.block_on(self.journal.rows(stream)).unwrap()
    }

    fn event(&mut self, event_id: &str) -> Option<StoredEvent> {
        self.db.block_on(self.journal.event(event_id)).unwrap()
    }

    fn property_cases(&self) -> u32 {
        24
    }
}
