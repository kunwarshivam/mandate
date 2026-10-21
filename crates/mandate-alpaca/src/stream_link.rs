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
//! subscribed and not down, and that instrument has had a two-sided quote and a status, each
//! received after the latest subscription (spec §3.2's `resyncing`, DP-5, DP-7; DEC-1174 item 9).
//! A status channel that never sends leaves its instruments blocked (DEC-1174 item 2). Nothing here
//! touches an exit: exits and protection never wait on the stream (DP-4, `AGENTS.md` rule 13).

use std::collections::BTreeSet;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_time::UtcNanos;

use crate::stream::{Channels, Disposition, Frame, StreamMessage, subscribe_frame};

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
    /// A link that will subscribe `channels`, alerting once it has been out of `live` for longer
    /// than `down_alert` since its first loss (the profile's staleness threshold, spec §3.2).
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

    /// The start of the gap not yet closed by a resubscription, for the connection's record: the
    /// last message received before the connection was lost or paused (spec §3.3).
    pub fn open_gap(&self) -> Option<UtcNanos> {
        self.gap_from
    }

    /// Whether an opening in `instrument` may use this connection's data now.
    pub fn opens(&self, instrument: &InstrumentId) -> bool {
        matches!(self.state, LinkState::Resyncing | LinkState::Live)
            && self.waited().contains(instrument)
            && self.quoted.contains(instrument)
            && self.statused.contains(instrument)
    }

    /// Folds one input received at `at`. `jitter_permille`, in `0..1000`, spreads a reconnect's
    /// delay over its upper half; it is read only when the input loses the connection.
    pub fn on(&mut self, at: UtcNanos, input: LinkInput, jitter_permille: u16) -> Vec<LinkAction> {
        let mut actions = Vec::new();
        if self.state == LinkState::Paused {
            return actions;
        }
        match input {
            LinkInput::Frame(frame) => self.frame(at, frame, jitter_permille, &mut actions),
            LinkInput::Lost => {
                if self.state != LinkState::Down {
                    self.down(at, jitter_permille, &mut actions);
                }
            }
            LinkInput::Reconnecting => {
                self.mark_gap();
                self.state = LinkState::Connecting;
                self.clear();
            }
            LinkInput::Tick => {
                let past = self
                    .lost_at
                    .is_some_and(|lost| elapsed(lost, at) > self.down_alert);
                if past && !self.alerted_down {
                    self.alerted_down = true;
                    actions.push(LinkAction::Alert("stream_down"));
                }
            }
        }
        actions
    }

    fn frame(&mut self, at: UtcNanos, frame: Frame, jitter: u16, actions: &mut Vec<LinkAction>) {
        if self.state != LinkState::Down {
            self.last_received = Some(at);
        }
        let subscribed = matches!(self.state, LinkState::Resyncing | LinkState::Live);
        let unreadable_fault = frame
            .refused
            .iter()
            .any(|refused| match refused.kind.as_deref() {
                Some("error") => self.state != LinkState::Down,
                Some("s" | "l") => subscribed,
                _ => false,
            });
        if unreadable_fault {
            self.down(at, jitter, actions);
            return;
        }
        for message in frame.messages {
            if let StreamMessage::Error(fault) = &message {
                match fault.disposition {
                    Disposition::Pause | Disposition::Narrow | Disposition::Down
                        if self.state == LinkState::Down =>
                    {
                        continue;
                    }
                    Disposition::Pause => {
                        self.pause("stream_credential_rejected", actions);
                        return;
                    }
                    Disposition::Narrow if self.channels == self.channels.narrowed() => {
                        self.pause("stream_subscription_invalid", actions);
                        return;
                    }
                    Disposition::Narrow => {
                        self.channels = self.channels.narrowed();
                        self.state = LinkState::Connecting;
                        self.clear();
                        self.subscribe(actions);
                        return;
                    }
                    Disposition::Overflow => {
                        actions.push(LinkAction::Alert("stream_symbol_limit"));
                        continue;
                    }
                    Disposition::Down => {
                        self.down(at, jitter, actions);
                        return;
                    }
                }
            }
            if self.state == LinkState::Down {
                continue;
            }
            match message {
                StreamMessage::Connected if self.state == LinkState::Connecting => {
                    actions.push(LinkAction::Authenticate);
                }
                StreamMessage::Authenticated if self.state == LinkState::Connecting => {
                    self.subscribe(actions);
                    if self.state == LinkState::Paused {
                        return;
                    }
                }
                StreamMessage::Subscribed(_) => {
                    self.state = LinkState::Resyncing;
                    self.clear();
                    self.subscribed_at = Some(at);
                    if let Some(from) = self.gap_from.take() {
                        actions.push(LinkAction::Gap { from, to: at });
                    }
                }
                StreamMessage::Quote(quote) if quote.bid.is_some() && quote.ask.is_some() => {
                    self.quoted.insert(quote.instrument);
                }
                StreamMessage::Status(status) => {
                    self.statused.insert(status.instrument);
                }
                _ => {}
            }
            if self.state == LinkState::Resyncing && self.resynced() {
                self.state = LinkState::Live;
                self.attempts = 0;
                self.lost_at = None;
                self.alerted_down = false;
            }
        }
    }

    fn down(&mut self, at: UtcNanos, jitter: u16, actions: &mut Vec<LinkAction>) {
        self.mark_gap();
        self.state = LinkState::Down;
        self.clear();
        if self.lost_at.is_none() {
            self.lost_at = Some(at);
        }
        actions.push(LinkAction::Reconnect {
            after: backoff(self.attempts, jitter),
        });
        self.attempts = self.attempts.saturating_add(1);
    }

    /// Ends the connection until its owner acts: no reconnect, openings blocked, and the gap open
    /// from the last message received (spec §3.2, §3.3).
    fn pause(&mut self, alert: &'static str, actions: &mut Vec<LinkAction>) {
        self.mark_gap();
        self.state = LinkState::Paused;
        self.clear();
        actions.push(LinkAction::Alert(alert));
    }

    /// Asks for the subscribe frame of the current channels, or pauses when nothing is left to
    /// subscribe, so the link never waits in `connecting` for an echo that cannot come.
    fn subscribe(&mut self, actions: &mut Vec<LinkAction>) {
        match subscribe_frame(&self.channels) {
            Ok(frame) => actions.push(LinkAction::Subscribe(frame)),
            Err(_) => self.pause("stream_subscription_invalid", actions),
        }
    }

    /// Opens a gap at the last message received, once a subscription has existed and none is
    /// already open: the window then runs to the next subscription (spec §3.3).
    fn mark_gap(&mut self) {
        if self.subscribed_at.is_some() && self.gap_from.is_none() {
            self.gap_from = self.last_received;
        }
    }

    fn clear(&mut self) {
        self.quoted.clear();
        self.statused.clear();
    }

    fn waited(&self) -> BTreeSet<InstrumentId> {
        self.channels
            .quotes
            .union(&self.channels.statuses)
            .cloned()
            .collect()
    }

    fn resynced(&self) -> bool {
        self.waited()
            .iter()
            .all(|i| self.quoted.contains(i) && self.statused.contains(i))
    }
}

/// The delay before reconnect attempt `attempt` (0 for the first), with `jitter_permille` of its
/// upper half: from half the doubled delay to all of it, never over [`MAX_BACKOFF`].
pub fn backoff(attempt: u32, jitter_permille: u16) -> Duration {
    let doubled = 2_u32
        .checked_pow(attempt)
        .and_then(|factor| FIRST_BACKOFF.checked_mul(factor))
        .map_or(MAX_BACKOFF, |delay| delay.min(MAX_BACKOFF));
    let half = doubled.checked_div(2).unwrap_or(Duration::ZERO);
    let spread = half
        .checked_mul(u32::from(jitter_permille.min(1000)))
        .and_then(|d| d.checked_div(1000))
        .unwrap_or(half);
    half.saturating_add(spread)
}

/// How long after `from` the instant `to` is, zero when it is not later.
fn elapsed(from: UtcNanos, to: UtcNanos) -> Duration {
    let nanos = |t: UtcNanos| {
        i128::from(t.secs())
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(t.nanos()))
    };
    u64::try_from(nanos(to).saturating_sub(nanos(from)))
        .map_or(Duration::ZERO, Duration::from_nanos)
}
