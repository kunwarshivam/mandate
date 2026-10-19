//! The market-data stream's connection states and reconnect (backlog E2-5 slice 2, data-plane
//! spec v0.3 §3.2 and §3.3, DEC-1174 items 2, 3 and 5), as a pure machine its transport drives.
//!
//! The transport opens and closes the socket and hands every received frame, already read by
//! [`crate::stream::read_frame`], to [`Link::on`] with the host-clock instant it was received at.
//! The link answers with what to send or do next. It never reads a clock or a random source: the
//! instant comes with every input and the jitter of each reconnect's delay with the input that
//! asks for it, so a run replays exactly.
//!
//! Openings in an instrument are allowed only while [`Link::opens`] says so: the connection is
//! subscribed and not down, and that instrument has had a live quote and a status, each received
//! after the latest subscription (spec §3.2's `resyncing`, DP-5, DP-7). A status channel that
//! never sends leaves its instruments blocked (DEC-1174 item 2). Nothing here touches an exit:
//! exits and protection never wait on the stream (DP-4, `AGENTS.md` rule 13).

use std::collections::BTreeSet;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_time::UtcNanos;

use crate::stream::{Channels, Frame};

/// The cap on a reconnect's delay (spec §3.2).
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// The first reconnect's delay, before jitter; each later one doubles, up to [`MAX_BACKOFF`].
pub const FIRST_BACKOFF: Duration = Duration::from_secs(1);

/// Where the connection is (spec §3.2's table, and the paused state of a rejected credential).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Opening the socket, authenticating and subscribing.
    Connecting,
    /// Subscribed; each instrument opens once it has a live quote and a status since.
    Resyncing,
    /// Every subscribed instrument resynced.
    Live,
    /// Disconnected, or a vendor error; the next reconnect is asked for.
    Down,
    /// A rejected credential or plan (402, 409): no reconnect until the owner acts, and openings
    /// stay blocked (spec §3.2, DEC-1174 item 5).
    Paused,
}

/// What the transport reports.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkInput {
    /// A frame received, already read.
    Frame(Frame),
    /// The socket closed, or a ping went unanswered: the connection is lost.
    Lost,
    /// The transport is about to reconnect, as the last [`LinkAction::Reconnect`] asked.
    Reconnecting,
    /// Time passed with nothing received: the link checks how long it has been down.
    Tick,
}

/// What the link asks of its transport and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// Send the auth frame now.
    Authenticate,
    /// Send this subscribe frame now.
    Subscribe(String),
    /// Close the socket, wait this long, then reconnect.
    Reconnect { after: Duration },
    /// A gap in the stream, from the last message received before the loss to the moment of
    /// resubscription (spec §3.3), for the backfill and the connection's record.
    Gap { from: UtcNanos, to: UtcNanos },
    /// An operator and owner alert, by reason code, with no instrument or price (rule 6).
    Alert(&'static str),
}

/// One stream connection, for one account and feed.
#[allow(dead_code, reason = "the stub of the tests commit (DEC-77)")]
#[derive(Debug, Clone)]
pub struct Link {
    channels: Channels,
    down_alert: Duration,
    state: LinkState,
    subscribed_at: Option<UtcNanos>,
    quoted: BTreeSet<InstrumentId>,
    statused: BTreeSet<InstrumentId>,
    last_received: Option<UtcNanos>,
    lost_at: Option<UtcNanos>,
    gap_from: Option<UtcNanos>,
    attempts: u32,
    alerted_down: bool,
}

impl Link {
    /// A link that will subscribe `channels`, alerting once it has been down for longer than
    /// `down_alert` (the profile's staleness threshold, spec §3.2).
    pub fn new(channels: Channels, down_alert: Duration) -> Self {
        Self {
            channels,
            down_alert,
            state: LinkState::Connecting,
            subscribed_at: None,
            quoted: BTreeSet::new(),
            statused: BTreeSet::new(),
            last_received: None,
            lost_at: None,
            gap_from: None,
            attempts: 0,
            alerted_down: false,
        }
    }

    pub fn state(&self) -> LinkState {
        self.state
    }

    /// Whether an opening in `instrument` may use this connection's data now.
    pub fn opens(&self, instrument: &InstrumentId) -> bool {
        let _ = instrument;
        false
    }

    /// Folds one input received at `at`. `jitter_permille`, in `0..1000`, spreads a reconnect's
    /// delay over its upper half; it is read only when the input loses the connection.
    pub fn on(&mut self, at: UtcNanos, input: LinkInput, jitter_permille: u16) -> Vec<LinkAction> {
        let _ = (at, input, jitter_permille, &self.channels, self.down_alert);
        Vec::new()
    }
}

/// The delay before reconnect attempt `attempt` (0 for the first), with `jitter_permille` of its
/// upper half: from half the doubled delay to all of it, never over [`MAX_BACKOFF`].
pub fn backoff(attempt: u32, jitter_permille: u16) -> Duration {
    let _ = (attempt, jitter_permille);
    Duration::ZERO
}
