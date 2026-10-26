//! The tests' workspace, its instruments, and their times.

use mandate_canon::Value;
use mandate_time::UtcNanos;

pub const WS: &str = "ws_1";

pub const AGENT: &str = "agent_a";

pub const ACCOUNT: &str = "01KDER00000000000000000001";

pub const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";

pub const QQQ: &str = "eb82ffb0-efd6-4f4e-9307-8fead28e04f6";

pub const AAPL: &str = "11111111-1111-4111-8111-111111111111";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// 2026-10-26T15:00:00Z, the tests' first step.
pub fn t0() -> UtcNanos {
    at("2026-10-26T15:00:00Z")
}

pub fn plus(t: UtcNanos, secs: i64) -> UtcNanos {
    UtcNanos::from_parts(t.secs() + secs, t.nanos()).unwrap()
}

pub fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(k, v)| (mandate_canon::Key::new(k).unwrap(), v.clone()))
            .collect(),
    )
}

pub fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}
