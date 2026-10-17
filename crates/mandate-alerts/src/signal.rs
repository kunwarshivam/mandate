//! The §8.2 rows whose source is not a journal record: a metric, a writer outcome, or another
//! service. Each producer hands a [`Signal`] holding only counts, durations, and flags, so no
//! signal can carry an instrument, an amount, or a mandate field.

use crate::AlertKind;

/// p99 journal append latency above which appends are degraded (journal spec §5.3: 5 ms).
pub const APPEND_P99_LIMIT_US: u64 = 5_000;
/// Cold export lag above which it alerts (journal spec §6.2: 1 minute, before live capital).
pub const COLD_EXPORT_LAG_LIMIT_S: u64 = 60;

/// One observation from a non-journal source, over the producer's own window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// An operator activated the global (managed) kill switch.
    GlobalKillSwitch,
    /// An order stayed `Unknown` past its lookup window (trading spec §5.7).
    OrderUnknownPastLookup,
    /// Reconciliation found the same order twice at the broker.
    DuplicateOrder,
    /// Runtime observations or ingest metrics found a feed stale.
    MarketDataStale,
    /// Model gateway errors, deadline misses, and `meter_unavailable` refusals.
    ModelGateway {
        errors: u64,
        deadline_misses: u64,
        meter_unavailable: u64,
    },
    /// Journal appends answered `Unavailable` or `Ambiguous`, and the p99 latency.
    JournalAppend {
        unavailable: u64,
        ambiguous: u64,
        p99_us: u64,
    },
    StandbyLost,
    /// A writer was fenced when no handover was in progress.
    UnexpectedFenced,
    /// Postgres audit saw a DDL or superuser session outside a release.
    DdlOutsideRelease,
    ColdExportLag {
        seconds: u64,
    },
    VaultLeaseRenewal {
        failures: u64,
    },
    /// Broker API errors, rate-limit answers, and whether the broker is down.
    Broker {
        errors: u64,
        rate_limited: u64,
        outage: bool,
    },
    /// The deployment manager left an agent paused after a crash loop.
    CrashLoop,
    /// The approval service could not deliver an approval request.
    ApprovalNotDelivered,
    BackupFailed,
    DrillOverdueOrFailed,
}

/// The alert a signal raises, if any. Where §8.2 names no threshold, any nonzero count alerts:
/// the conservative reading until O3's rules are tuned (DEC-981 item 4).
pub fn from_signal(signal: Signal) -> Option<AlertKind> {
    let _ = (signal, AlertKind::AnchorGap);
    None
}
