//! A broker and the wiring that belongs with it (E21-30, DEC-2359 item 9). The fixture broker
//! serves with the fixture's wiring, whose quote feed reads the fixture's own quotes; a broker
//! whose transport reads market data serves with the paper wiring, whose quote feed reads over
//! that same transport. Each run carries the facts its broker states ([`crate::facts`], G2b-2),
//! which [`Run::deployed`] hands the binding gate. Only [`fixture`] and [`paper`] make a [`Run`],
//! and each takes only its own kind of broker, so the serve path cannot pair the paper broker with
//! a wiring that reads no quote, or a broker with another's facts.

use mandate_alpaca::{DataTransport, Pause, TokioPause, TradingTransport};
use mandate_executor::AccountScope;

use crate::Wiring;
use crate::alpaca::AlpacaBroker;
use crate::credential::paper_wiring;
use crate::deployment::{BrokerFacts, Deployment, Named, Production, Unread};
use crate::facts::{alpaca_facts, fixture_facts};
use crate::paper::FixturePaper;

/// A broker paired with its wiring, which only [`fixture`] and [`paper`] make.
pub struct Run<B> {
    broker: B,
    wiring: Wiring,
    facts: Option<Box<dyn BrokerFacts>>,
}

impl<B> Run<B> {
    /// The wiring the run serves with.
    pub fn wiring(&self) -> &Wiring {
        &self.wiring
    }

    /// The broker's facts the run will hand its deployment, until [`Run::deployed`] hands them;
    /// `None` after.
    pub fn facts(&self) -> Option<&dyn BrokerFacts> {
        self.facts.as_deref()
    }

    /// The run with its wiring's mandate, instrument, protection, binding-gate and marking ports
    /// read from `scope`'s deployments in `production` ([`Deployment::wired`]), in place of the
    /// fixture's, which know nothing; its quote feed stays the broker's, and the broker's facts are
    /// the run's own. Its broker's answers name each deployed instrument by its asset id
    /// ([`Named`]).
    pub fn deployed(self, scope: &AccountScope, production: Production) -> Run<Named<B>> {
        let Production {
            rows,
            artifacts,
            liquidity,
        } = production;
        let Self {
            broker,
            wiring,
            facts,
        } = self;
        let facts = facts.unwrap_or_else(|| Box::new(Unread));
        let (wiring, deployment) =
            Deployment::wired(wiring, scope, rows, artifacts, (liquidity, facts));
        Run {
            broker: Named::new(broker, Some(deployment)),
            wiring,
            facts: None,
        }
    }

    /// The run as it serves with no deployment read: the fixture's ports, which know nothing, and
    /// its broker's answers as the broker gave them.
    pub fn undeployed(self) -> Run<Named<B>> {
        let Self {
            broker,
            wiring,
            facts,
        } = self;
        Run {
            broker: Named::new(broker, None),
            wiring,
            facts,
        }
    }

    /// The broker and its wiring, for the serve path.
    pub fn into_parts(self) -> (B, Wiring) {
        (self.broker, self.wiring)
    }
}

/// The fixture broker with the fixture's wiring, its quote feed over the fixture's own quotes
/// judged on the host's clock ([`crate::credential::paper_wiring`]), and the fixture's facts. A
/// fixture that quotes nothing ([`FixturePaper::quoting`] not called) answers no quote, so the
/// core is handed none and the gate holds every opening.
///
/// # Errors
/// The paper wiring's refusal, and `runtime_unavailable`.
pub fn fixture(
    broker: AlpacaBroker<FixturePaper>,
    scope: &AccountScope,
) -> Result<Run<AlpacaBroker<FixturePaper>>, &'static str> {
    let wiring = paper_wiring(&broker, TokioPause, scope)?;
    let facts: Box<dyn BrokerFacts> = Box::new(fixture_facts(&broker)?);
    Ok(Run {
        broker,
        wiring,
        facts: Some(facts),
    })
}

/// A broker whose transport reads market data, with the paper wiring: its quote feed reads over
/// that transport, judged on `clock` ([`crate::credential::paper_wiring`]).
///
/// # Errors
/// The paper wiring's refusal.
pub fn paper<T, P>(
    broker: AlpacaBroker<T>,
    clock: P,
    scope: &AccountScope,
) -> Result<Run<AlpacaBroker<T>>, &'static str>
where
    T: TradingTransport + DataTransport + Clone + 'static,
    P: Pause + 'static,
{
    let wiring = paper_wiring(&broker, clock, scope)?;
    let facts: Box<dyn BrokerFacts> = Box::new(alpaca_facts(&broker)?);
    Ok(Run {
        broker,
        wiring,
        facts: Some(facts),
    })
}
