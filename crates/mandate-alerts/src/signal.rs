//! The §8.2 rows whose source is not a journal record: a metric, a writer outcome, or another
//! service. Each producer hands a [`Signal`] holding only counts, durations, and flags, so no
//! signal can carry an instrument, an amount, or a mandate field.

use crate::AlertKind;

/// p99 journal append latency above which appends are degraded (journal spec §5.3: 5 ms).
pub const APPEND_P99_LIMIT_US: u64 = 5_000;
/// Stale feeds at or above which a stale feed is SEV-2 "at scale" (§8.2 names no number; two is
/// the conservative reading, DEC-1070 item 4).
pub const STALE_FEEDS_AT_SCALE: u64 = 2;
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
    /// Runtime observations or ingest metrics found this many feeds stale.
    MarketDataStale {
        feeds: u64,
    },
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
/// the conservative reading until O3's rules are tuned (DEC-1070 item 4).
pub fn from_signal(signal: Signal) -> Option<AlertKind> {
    let raised = |fires: bool, kind: AlertKind| fires.then_some(kind);
    match signal {
        Signal::GlobalKillSwitch => Some(AlertKind::GlobalKillSwitch),
        Signal::OrderUnknownPastLookup => Some(AlertKind::OrderUnknownPastLookup),
        Signal::DuplicateOrder => Some(AlertKind::DuplicateOrder),
        Signal::MarketDataStale { feeds: 0 } => None,
        Signal::MarketDataStale { feeds } if feeds >= STALE_FEEDS_AT_SCALE => {
            Some(AlertKind::MarketDataStaleAtScale)
        }
        Signal::MarketDataStale { .. } => Some(AlertKind::MarketDataStale),
        Signal::ModelGateway {
            errors,
            deadline_misses,
            meter_unavailable,
        } => raised(
            errors > 0 || deadline_misses > 0 || meter_unavailable > 0,
            AlertKind::ModelGatewayDegraded,
        ),
        Signal::JournalAppend {
            unavailable,
            ambiguous,
            p99_us,
        } => raised(
            unavailable > 0 || ambiguous > 0 || p99_us > APPEND_P99_LIMIT_US,
            AlertKind::JournalAppendDegraded,
        ),
        Signal::StandbyLost => Some(AlertKind::StandbyLost),
        Signal::UnexpectedFenced => Some(AlertKind::UnexpectedFenced),
        Signal::DdlOutsideRelease => Some(AlertKind::DdlOutsideRelease),
        Signal::ColdExportLag { seconds } => {
            raised(seconds > COLD_EXPORT_LAG_LIMIT_S, AlertKind::ColdExportLag)
        }
        Signal::VaultLeaseRenewal { failures } => {
            raised(failures > 0, AlertKind::VaultLeaseRenewalFailing)
        }
        Signal::Broker {
            errors,
            rate_limited,
            outage,
        } => raised(
            errors > 0 || rate_limited > 0 || outage,
            AlertKind::BrokerDegraded,
        ),
        Signal::CrashLoop => Some(AlertKind::CrashLoop),
        Signal::ApprovalNotDelivered => Some(AlertKind::ApprovalNotDelivered),
        Signal::BackupFailed => Some(AlertKind::BackupFailed),
        Signal::DrillOverdueOrFailed => Some(AlertKind::DrillOverdueOrFailed),
    }
}
