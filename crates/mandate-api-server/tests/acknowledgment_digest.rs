//! The acknowledgment routes' step-up digest (§3.11 item 12, DEC-2144): the spec's worked vectors
//! are checked against the object the HTTP tests bind. The holds fold the routes read is
//! `mandate_journal::holds`, tested in `mandate-journal`.

use mandate_canon::{Digest, parse, to_canonical};

/// §3.11 item 12's object for `confirms` of the hold, member by member in canonical order, as
/// `versions_http`'s challenges bind it, with `control_head` 42.
fn bound(confirms: &str, request: Option<&str>) -> String {
    let request = request.map_or(String::new(), |id| format!(r#""request":"{id}","#));
    let object = format!(
        r#"{{"agent":"agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z","bid":null,"bid_size":null,"confirms":"{confirms}","control_head":"42","event":"01JBH3BTZ8K2M4N6P8R0S2T4V6","floor":null,"instrument":null,{request}"subject":"reconciliation"}}"#
    );
    let value = parse(object.as_bytes()).expect("JSON");
    format!("sha256:{}", Digest::of(&to_canonical(&value)).to_hex())
}

#[test]
fn the_bound_object_reproduces_the_spec_s_three_vectors() {
    assert_eq!(
        bound("acknowledge", None),
        "sha256:5f5506a3ac32cae637a2cc222393f852035c699430656870ba024702431260a7"
    );
    assert_eq!(
        bound("acknowledge", Some("01JBH3BTZ8K2M4N6P8R0S2T4V7")),
        "sha256:b9367c62d1201059c5a9f2cf0bf35e637d27a5934e174b779276a2fcbc7af765"
    );
    assert_eq!(
        bound("request_ack", None),
        "sha256:b15c59e4e09394ddf0f5430c04569276c27c7a957ac68c683e53b15e6b393ebc"
    );
}
