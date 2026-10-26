//! The broker's and the facts' runtimes drive IO as well as time (lane X's review of #2561): the
//! paper host's transport is a socket, and a current-thread runtime built without the IO driver
//! panics on its first read, inside the process or the binding-gate port. A transport that opens a
//! local socket stands in for it here; nothing leaves the machine.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::thread;

use mandate_alpaca::{HttpRequest, Response, TradingTransport, TransportError};
use mandate_executor::BrokerRequest;
use mandate_executord::Broker;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::deployment::BrokerFacts;
use mandate_executord::facts::alpaca_facts;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Error = Box<dyn std::error::Error>;

/// A transport that answers every request over a loopback socket, with the body a local server
/// sends back: one that needs tokio's IO driver.
#[derive(Clone)]
struct Loopback(SocketAddr);

impl TradingTransport for Loopback {
    async fn send(&self, _request: &HttpRequest) -> Result<Response, TransportError> {
        let mut socket = tokio::net::TcpStream::connect(self.0)
            .await
            .map_err(|_| TransportError::Connect)?;
        socket
            .write_all(b"ask")
            .await
            .map_err(|_| TransportError::Request)?;
        let mut body = Vec::new();
        socket
            .read_to_end(&mut body)
            .await
            .map_err(|_| TransportError::Request)?;
        Ok(Response { status: 404, body })
    }
}

/// A local server that answers `count` connections with a not-found body.
fn serve(count: usize) -> Result<SocketAddr, Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    thread::spawn(move || {
        for stream in listener.incoming().take(count).flatten() {
            let mut stream = stream;
            let mut ask = [0_u8; 3];
            let _asked = stream.read_exact(&mut ask);
            let _answered = stream.write_all(br#"{"code":40410000,"message":"not found"}"#);
        }
    });
    Ok(addr)
}

/// The broker's call and the facts' reads each cross a socket and come back with the server's
/// answer, an unreadable account and no asset, rather than a panic for want of the IO driver.
#[test]
fn the_broker_and_its_facts_drive_a_socket() -> Result<(), Error> {
    let addr = serve(3)?;
    let mut broker = AlpacaBroker::new(Loopback(addr))?;
    assert!(broker.call(&BrokerRequest::GetAccount).is_err());
    let facts = alpaca_facts(&broker)?;
    assert_eq!(facts.asset("SPY"), None);
    assert_eq!(facts.rules(), None);
    Ok(())
}
