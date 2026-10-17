//! The retrieval plan (§6.3) and the scope the harness, never the model, gives every read.

use std::collections::BTreeSet;

use mandate_time::UtcNanos;

use crate::item::{Allowlist, Symbol};

/// The fixed list of reads a run makes, with their caps. It is part of the research entry, so the
/// owner's pin covers it (DEC-431 item 5): changing a cap is a new research-agent version.
///
/// Only the caps vary. Which reads exist is fixed by this type having no other field: there is no
/// URL, no search, and no read of another agent's or workspace's data to configure (§6.3, "There
/// is nothing else").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalPlan {
    /// Daily bars per data-universe instrument, ending at or before the cut-off.
    pub bars_per_instrument: u32,
    /// How far back a news item's knowledge time may be, in seconds before the cut-off.
    pub news_window_s: u32,
    pub news_max_items: u32,
    /// The bytes of headline and summary together that one news item may carry into the prompt.
    pub news_item_bytes: u32,
    pub filings_window_s: u32,
    pub filings_max_items: u32,
    pub filing_excerpt_bytes: u32,
    /// The retrieval's own wall-clock bound; the run's whole bound is this plus the call deadline
    /// (§7.1).
    pub retrieval_timeout_s: u32,
}

impl RetrievalPlan {
    /// §6.3's and §7.1's defaults (DEC-431 item 7): 20 bars, 48 hours and 50 items of news at
    /// 2,000 bytes, 7 days and 20 filings at 4,000 bytes, and a 60 s retrieval bound.
    pub const fn v0_1() -> Self {
        Self {
            bars_per_instrument: 20,
            news_window_s: 172_800,
            news_max_items: 50,
            news_item_bytes: 2_000,
            filings_window_s: 604_800,
            filings_max_items: 20,
            filing_excerpt_bytes: 4_000,
            retrieval_timeout_s: 60,
        }
    }
}

/// Who and when a run reads for. Every read takes its scope from here (§6.3): the model names no
/// read, so an injected instruction to read another tenant's data has nowhere to go (§9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub workspace: String,
    pub agent: String,
    /// The run's cut-off (`as_of`): no item whose knowledge time is after it is kept (DP-1).
    pub cut_off: UtcNanos,
    /// The instruments research may read about (§1.3's data universe).
    pub data_universe: BTreeSet<Symbol>,
    /// The agent's held instruments, which the filings read covers as well (§6.3).
    pub held: BTreeSet<Symbol>,
    pub allowlist: Allowlist,
}
