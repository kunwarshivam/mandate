//! The opening's quote cross-check (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1b;
//! [DEC-529](../../../docs/project/decisions/DEC-529.md) item 12; trading-domain spec §4.2's
//! cross-checked row): the collar and the risk mark read the broker's own quote (the ask
//! `review_equity_order` re-reads), and before the order leaves it must agree with the
//! independent IEX quote within the collar, or the opening is refused and nothing is sent
//! ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 2). Neither quote is read here:
//! the runner hands both asks in, so the seam holds no host and no credential (ES-19, LT-1).

use mandate_num::{Adverse, Fraction, Price};

/// Why the opening's cross-check refuses ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 2): each refusal is a stop before any submission, each with its own reason code
/// (ADR-0001 ES-09).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuoteRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// A quote the cross-check reads is missing: an opening already requires a fresh IEX quote
    /// (trading-domain spec §4.2), and a check that cannot be made is not passed (rule 3).
    #[error("a quote the cross-check reads is missing")]
    Absent,
    /// The broker's ask disagrees with the independent ask beyond the collar: the bad tick the
    /// cross-check exists for (the brief's adversary row), refused before any submission.
    #[error("the broker's quote disagrees with the independent quote beyond the collar")]
    Disagreement,
}

impl QuoteRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `quote_absent`,
    /// `quote_disagreement`, and the stub's `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Absent => "quote_absent",
            Self::Disagreement => "quote_disagreement",
        }
    }
}

/// DEC-529 item 12's cross-check ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 2):
/// whether the opening's quotes agree. The broker's ask — the side a buy opening prices against,
/// the ask `review_equity_order` re-reads — must sit inside the closed band the independent
/// IEX ask prices at `collar`, the collar's own aggressiveness for the asset class (trading-domain
/// spec §9.6's table, the fraction the gate's collar reads), each end computed by
/// `Price::collar_bound` itself — one rounding, once, at the 9 places a price holds: truncation
/// toward zero on the ceiling, ceiling on the floor — so the band is only ever narrower
/// ([DEC-163](../../../docs/project/04-decision-log.md#decisions)
/// item 2) and no price the collar would refuse can pass here. The agreement is symmetric: a
/// broker quote that jumps either way refuses, and a missing broker or independent quote refuses
/// the same way (rule 3); a band whose ends the collar cannot price — a floor rounded to no
/// positive price — is no band the broker's ask can be shown to agree within, so the check that
/// cannot be made is not passed and the opening refuses as a disagreement (rule 3). Both quotes
/// are decimal strings parsed by `mandate-num` before they arrive (LT-12).
///
/// # Errors
/// Any [`QuoteRefusal`] that applies, before any submission.
pub fn cross_check(
    broker_ask: Option<Price>,
    independent_ask: Option<Price>,
    collar: Fraction,
) -> Result<(), QuoteRefusal> {
    let (Some(broker), Some(independent)) = (broker_ask, independent_ask) else {
        return Err(QuoteRefusal::Absent);
    };
    let band = independent
        .collar_bound(collar, Adverse::Up)
        .and_then(|ceiling| {
            independent
                .collar_bound(collar, Adverse::Down)
                .map(|floor| (ceiling, floor))
        });
    let (ceiling, floor) = band.map_err(|_| QuoteRefusal::Disagreement)?;
    if broker > ceiling || broker < floor {
        return Err(QuoteRefusal::Disagreement);
    }
    Ok(())
}
