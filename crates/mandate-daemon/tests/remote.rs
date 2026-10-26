//! The runtime's side of the model-gateway service (DEC-2397 items 1 and 2, DEC-2265 items 1 to 4):
//! a record or a refusal crosses whole; every exchange lost before the whole reply for this call
//! arrives, by the request's deadline, is `gateway_unreachable`; and a lost call is never sent
//! again. Each case runs against a real local socket that a test thread scripts, and the oracle
//! counts the connections and bytes that socket saw.

#![allow(
    clippy::disallowed_methods,
    reason = "each case times a real socket against the host clock, as the runtime's edge does"
)]

use std::collections::BTreeMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mandate_canon::Digest;
use mandate_daemon::remote::{GRACE, RemoteGateway};
use mandate_gateway::frame::{MAX_REPLY_FRAME, MAX_REQUEST_FRAME, read_frame, write_frame};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::registry::ModelRef;
use mandate_gateway::wire::{Reply, reply_bytes, request_from};
use mandate_gateway::{CallRecord, CallRequest, RequestError};
use mandate_research_run::ModelCalls;
use mandate_time::UtcNanos;

fn host_now() -> UtcNanos {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    UtcNanos::from_parts(
        i64::try_from(since.as_secs()).unwrap(),
        since.subsec_nanos(),
    )
    .unwrap()
}

fn fixed() -> UtcNanos {
    UtcNanos::from_parts(1_792_159_200, 0).unwrap()
}

/// A request whose deadline is `ms` milliseconds from now on the host clock.
fn request(ms: u64) -> CallRequest {
    let now = host_now();
    let deadline_ns = u64::from(now.nanos()) + ms * 1_000_000;
    let deadline = UtcNanos::from_parts(
        now.secs() + i64::try_from(deadline_ns / 1_000_000_000).unwrap(),
        u32::try_from(deadline_ns % 1_000_000_000).unwrap(),
    )
    .unwrap();
    CallRequest {
        call_id: mandate_canon::encode_ulid(1 << 90),
        workspace_id: "ws_1".to_owned(),
        agent_id: Some("agent-a".to_owned()),
        purpose: Purpose::Research,
        model: ModelRef {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"model"),
        },
        inputs: BTreeMap::from([("bars".to_owned(), "SPY 2026-10-23 close 150".to_owned())]),
        as_of: now,
        deadline,
        observations: vec![],
        key_owner: KeyOwner::Platform,
        run_kind: None,
        turns: vec![],
        tools: vec![],
        spend_ceiling: None,
    }
}

/// The record the gateway would give `request`, built here member by member.
fn record_for(request: &CallRequest) -> CallRecord {
    CallRecord {
        call_id: request.call_id.clone(),
        agent_id: request.agent_id.clone(),
        purpose: request.purpose,
        model: request.model.clone(),
        endpoint: "recorded:demo".to_owned(),
        request_digest: Some(Digest::of(b"request")),
        guard_rule: None,
        seed: Some(7),
        prompt_ref: Some(Digest::of(b"prompt")),
        response_ref: Some(Digest::of(b"response")),
        reported_identity: Some("recorded/model@Recorded".to_owned()),
        provider_request_id: None,
        outcome: Outcome::Ok,
        attempts: 1,
        tokens: Usage {
            input: 100,
            output: 20,
            cached: 0,
        },
        cost: Usd6::parse("0.0006").unwrap(),
        price_table_ref: Digest::of(b"prices"),
        cache_hit: false,
        deadline: request.deadline,
        completed_at: request.as_of,
        reservation: Some("01KDEMRESERVATION000000000".to_owned()),
        observations: vec![],
        output: None,
        tool_calls: vec![],
        prompt: Some(b"prompt".to_vec()),
        response: Some(b"response".to_vec()),
    }
}

/// What the scripted service does with one connection.
enum Script {
    /// Reads the request and answers with what `reply` gives it.
    Answer(fn(&CallRequest) -> Reply),
    /// Reads the request, then sends nothing and holds the connection.
    Stall,
    /// Reads the request, then answers its record half a second past its deadline.
    Late,
    /// Reads the request, then sends one byte of a valid reply every 50 ms.
    Trickle,
    /// Reads the request, then sends a length prefix and only part of its body, and closes.
    Cut,
    /// Reads the request, then sends a length prefix past the reply bound.
    Oversized,
    /// Reads the request, then sends a well-framed body that is not a reply.
    Malformed,
}

/// A scripted service on a local port, counting the connections it accepts.
struct Service {
    address: SocketAddr,
    accepted: Arc<AtomicUsize>,
    handle: JoinHandle<()>,
}

fn serve(script: Script) -> Service {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&accepted);
    let handle = thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(6) {
            match listener.accept() {
                Ok((stream, _)) => {
                    counted.fetch_add(1, Ordering::SeqCst);
                    stream.set_nonblocking(false).unwrap();
                    run(stream, &script);
                    thread::sleep(Duration::from_millis(200));
                    if listener.accept().is_ok() {
                        counted.fetch_add(1, Ordering::SeqCst);
                    }
                    return;
                }
                Err(_) => thread::sleep(Duration::from_millis(5)),
            }
        }
    });
    Service {
        address,
        accepted,
        handle,
    }
}

fn run(mut stream: TcpStream, script: &Script) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let Ok(body) = read_frame(&mut stream, MAX_REQUEST_FRAME) else {
        return;
    };
    let request = request_from(&body).unwrap();
    match script {
        Script::Answer(reply) => {
            write_frame(&mut stream, &reply_bytes(&reply(&request)), MAX_REPLY_FRAME).unwrap();
        }
        Script::Stall => thread::sleep(GRACE + Duration::from_secs(2)),
        Script::Late => {
            thread::sleep(Duration::from_millis(800));
            write_frame(
                &mut stream,
                &reply_bytes(&Ok(record_for(&request))),
                MAX_REPLY_FRAME,
            )
            .unwrap();
        }
        Script::Trickle => {
            let mut frame = Vec::new();
            write_frame(
                &mut frame,
                &reply_bytes(&Ok(record_for(&request))),
                MAX_REPLY_FRAME,
            )
            .unwrap();
            for byte in frame {
                if stream.write_all(&[byte]).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        Script::Cut => {
            let _ = stream.write_all(&100_u32.to_be_bytes());
            let _ = stream.write_all(&[b'{'; 10]);
        }
        Script::Oversized => {
            let _ = stream.write_all(&(MAX_REPLY_FRAME + 1).to_be_bytes());
            let _ = stream.write_all(&[0; 64]);
        }
        Script::Malformed => {
            write_frame(&mut stream, b"{\"wire_version\":99}", MAX_REPLY_FRAME).unwrap();
        }
    }
}

fn gateway(service: &Service) -> RemoteGateway {
    RemoteGateway::new(service.address, host_now)
}

/// DEC-2265 items 1 and 6: the record comes back whole, as the service sent it, over one
/// connection.
#[test]
#[ignore = "pending E21-62"]
fn a_record_crosses_whole_over_one_connection() {
    let service = serve(Script::Answer(|request| Ok(record_for(request))));
    let request = request(2_000);
    assert_eq!(gateway(&service).call(&request), Ok(record_for(&request)));
    service.handle.join().unwrap();
    assert_eq!(service.accepted.load(Ordering::SeqCst), 1);
}

/// DEC-2265 item 5: the gateway's own refusal comes back as itself, not as a lost exchange.
#[test]
#[ignore = "pending E21-62"]
fn the_gateway_s_refusal_comes_back_as_itself() {
    let service = serve(Script::Answer(|_| Err(RequestError::UnknownModel)));
    assert_eq!(
        gateway(&service).call(&request(2_000)),
        Err(RequestError::UnknownModel)
    );
    service.handle.join().unwrap();
}

/// DEC-2397 item 2: a record for another call is no answer to this one.
#[test]
#[ignore = "pending E21-62"]
fn a_record_for_another_call_is_unreachable() {
    let service = serve(Script::Answer(|request| {
        let mut other = record_for(request);
        other.call_id = mandate_canon::encode_ulid((1 << 90) + 1);
        Ok(other)
    }));
    assert_eq!(
        gateway(&service).call(&request(2_000)),
        Err(RequestError::GatewayUnreachable)
    );
    service.handle.join().unwrap();
}

/// DEC-2397 item 2: a refused connect is unreachable.
#[test]
#[ignore = "pending E21-62"]
fn a_refused_connect_is_unreachable() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = closed.local_addr().unwrap();
    drop(closed);
    assert_eq!(
        RemoteGateway::new(address, host_now).call(&request(2_000)),
        Err(RequestError::GatewayUnreachable)
    );
}

/// DEC-2265 item 4, DEC-2397 items 1 and 2: a reply that does not come is unreachable at the
/// deadline and its grace, not before and not long after, and the call is sent once.
#[test]
#[ignore = "pending E21-62"]
fn a_stalled_reply_is_unreachable_at_the_deadline() {
    let service = serve(Script::Stall);
    let started = Instant::now();
    assert_eq!(
        gateway(&service).call(&request(300)),
        Err(RequestError::GatewayUnreachable)
    );
    let took = started.elapsed();
    assert!(took >= GRACE + Duration::from_millis(250), "{took:?}");
    assert!(took < GRACE + Duration::from_millis(900), "{took:?}");
    service.handle.join().unwrap();
    assert_eq!(
        service.accepted.load(Ordering::SeqCst),
        1,
        "never sent again"
    );
}

/// DEC-2397 item 1: a reply the service finishes just past the request's deadline, inside the
/// grace, is read whole rather than lost to the two clocks.
#[test]
#[ignore = "pending E21-62"]
fn a_reply_just_past_the_deadline_is_read_inside_the_grace() {
    let service = serve(Script::Late);
    let request = request(300);
    assert_eq!(gateway(&service).call(&request), Ok(record_for(&request)));
    service.handle.join().unwrap();
}

/// DEC-2265 item 4: the bound is absolute, so a reply trickled a byte at a time, each well inside
/// one read's timeout, still ends at the deadline and its grace.
#[test]
#[ignore = "pending E21-62"]
fn a_trickled_reply_still_ends_at_the_deadline() {
    let service = serve(Script::Trickle);
    let started = Instant::now();
    assert_eq!(
        gateway(&service).call(&request(400)),
        Err(RequestError::GatewayUnreachable)
    );
    let took = started.elapsed();
    assert!(took >= GRACE + Duration::from_millis(300), "{took:?}");
    assert!(took < GRACE + Duration::from_millis(1_000), "{took:?}");
    service.handle.join().unwrap();
}

/// DEC-2265 item 1: a reply cut short, one past its bound, or one that does not decode is
/// unreachable, each on one connection.
#[test]
#[ignore = "pending E21-62"]
fn a_cut_oversized_or_malformed_reply_is_unreachable() {
    for script in [Script::Cut, Script::Oversized, Script::Malformed] {
        let service = serve(script);
        assert_eq!(
            gateway(&service).call(&request(1_000)),
            Err(RequestError::GatewayUnreachable)
        );
        service.handle.join().unwrap();
        assert_eq!(service.accepted.load(Ordering::SeqCst), 1);
    }
}

/// DEC-2397 item 2: a request whose deadline has already passed is unreachable without a
/// connection.
#[test]
#[ignore = "pending E21-62"]
fn a_request_past_its_deadline_opens_no_connection() {
    let service = serve(Script::Answer(|request| Ok(record_for(request))));
    let mut late = request(1_000);
    late.deadline = late.as_of;
    assert_eq!(
        gateway(&service).call(&late),
        Err(RequestError::GatewayUnreachable)
    );
    service.handle.join().unwrap();
    assert_eq!(service.accepted.load(Ordering::SeqCst), 0);
}

/// DEC-2397 item 1: a call is built at the clock the remote gateway was given.
#[test]
#[ignore = "pending E21-62"]
fn calls_are_built_at_the_given_clock() {
    let closed: SocketAddr = "127.0.0.1:9".parse().unwrap();
    assert_eq!(RemoteGateway::new(closed, fixed).now(), fixed());
}

/// The request frame the service reads is the request, whole.
#[test]
#[ignore = "pending E21-62"]
fn the_service_reads_the_request_whole() {
    fn echo_inputs(request: &CallRequest) -> Reply {
        let mut record = record_for(request);
        record.observations = request.inputs.values().cloned().collect();
        Ok(record)
    }
    let service = serve(Script::Answer(echo_inputs));
    let request = request(2_000);
    let record = gateway(&service).call(&request).unwrap();
    assert_eq!(record.observations, ["SPY 2026-10-23 close 150"]);
    service.handle.join().unwrap();
}
