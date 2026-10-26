//! The process's [`Broker`] over `mandate-alpaca`'s client (DEC-2007 item 2): the same client the
//! shell drives, over whatever transport it is given. This crate names no host and builds no URL;
//! the dev and CI cells hand it [`crate::paper::FixturePaper`].
//!
//! The client's call is async and the process is not, so the adapter owns the runtime that drives
//! it: one current-thread tokio runtime, built once, that holds nothing a restart would lose. It
//! drives IO as well as time, which the paper host's HTTP transport needs.

use mandate_alpaca::{
    DeclaredRegime as AlpacaRegime, RetryPolicy, TokioPause, TradingClient, TradingTransport,
    alpaca_account_rules,
};
use mandate_connections::checks::{AccountRead, CheckReport, Occasion};
use mandate_connections::record::ConnectionId;
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountDeclaration, BrokerConnector, BrokerOutcome, BrokerRequest, ConnectorError,
    DeclaredRegime,
};
use tokio::runtime::Runtime;

use crate::Broker;

/// Alpaca's paper trading API over `transport`.
pub struct AlpacaBroker<T> {
    transport: T,
    runtime: Runtime,
}

impl<T: TradingTransport + Clone> AlpacaBroker<T> {
    /// The broker over `transport`.
    ///
    /// # Errors
    /// `runtime_unavailable` when the runtime that drives the client cannot be built.
    pub fn new(transport: T) -> Result<Self, &'static str> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        Ok(Self { transport, runtime })
    }

    /// A copy of the transport, with its credential, for a reader beside the broker: the quote
    /// feed's (DEC-2359 item 2).
    pub fn transport(&self) -> T {
        self.transport.clone()
    }

    fn client(&self) -> TradingClient<T, TokioPause> {
        TradingClient::new(self.transport.clone(), TokioPause, RetryPolicy::default())
    }
}

impl<T: TradingTransport + Clone> Broker for AlpacaBroker<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        self.client().profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let mut client = self.client();
        self.runtime.block_on(client.call(request))
    }

    fn has_later_checks(&self) -> bool {
        true
    }

    /// The connect check's own §8.1 checks, at every start and daily (DEC-2840).
    fn later_check(
        &self,
        connection: ConnectionId,
        occasion: Occasion,
        account: AccountRead,
    ) -> Option<Result<CheckReport, &'static str>> {
        Some(crate::connect::alpaca_report(connection, occasion, account))
    }

    /// Alpaca's declaration (trading spec §7.2, DEC-840): always margin, under `intraday_margin`.
    fn declared(&self) -> Option<AccountDeclaration> {
        let rules = alpaca_account_rules();
        Some(AccountDeclaration {
            account_type: rules.account_type,
            regime: match rules.regime {
                AlpacaRegime::IntradayMargin => DeclaredRegime::IntradayMargin,
                AlpacaRegime::LegacyPdt => DeclaredRegime::LegacyPdt,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use mandate_accounting::AccountType;
    use mandate_executor::{AccountDeclaration, DeclaredRegime};
    use mandate_time::UtcNanos;

    use mandate_connections::checks::{AccountRead, Occasion};
    use mandate_connections::record::ConnectionId;

    use super::AlpacaBroker;
    use crate::Broker;
    use crate::paper::FixturePaper;

    /// DEC-2840: Alpaca's broker runs the later permission checks, the connect check's own, on
    /// the occasion it is asked.
    #[test]
    fn alpacas_broker_runs_the_later_checks() -> Result<(), Box<dyn std::error::Error>> {
        let broker = AlpacaBroker::new(FixturePaper::new(UtcNanos::EPOCH))?;
        assert!(broker.has_later_checks());
        let report = broker
            .later_check(
                ConnectionId::new("conn-1")?,
                Occasion::Daily,
                AccountRead::Unreadable,
            )
            .ok_or("no later check")??;
        assert_eq!(report.occasion, Occasion::Daily);
        assert!(report.account_pii_ref.is_none());
        Ok(())
    }

    /// Alpaca's broker declares Alpaca's rules (trading spec §7.2: always margin, under
    /// `intraday_margin`; DEC-840), which every snapshot it reconciles journals (journal spec
    /// §9.35, DEC-2398), whatever transport it runs over.
    #[test]
    fn alpacas_broker_declares_a_margin_account_under_intraday_margin()
    -> Result<(), Box<dyn std::error::Error>> {
        let broker = AlpacaBroker::new(FixturePaper::new(UtcNanos::EPOCH))?;
        assert_eq!(
            broker.declared(),
            Some(AccountDeclaration {
                account_type: AccountType::Margin,
                regime: DeclaredRegime::IntradayMargin,
            })
        );
        Ok(())
    }
}
