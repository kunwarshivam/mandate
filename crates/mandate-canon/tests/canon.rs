//! Canonical JSON (journal spec §4). Oracles: `serde_json_canonicalizer` (an independent RFC 8785
//! implementation) and a scrambling writer that reorders members, adds whitespace, and escapes every
//! non-ASCII character.

use mandate_canon::{
    Digest, Int, Key, MAX_DEPTH, MAX_INT, Object, ParseErrorKind, Value, parse, to_canonical,
};
use proptest::collection::{btree_map, vec};
use proptest::prelude::*;

fn kind(input: &str) -> Option<ParseErrorKind> {
    parse(input.as_bytes()).err().map(|e| e.kind)
}

fn canonical(input: &str) -> String {
    String::from_utf8(to_canonical(&parse(input.as_bytes()).unwrap())).unwrap()
}

#[test]
#[ignore = "pending E5-1"]
fn string_escaping_vector() {
    // journal.yaml string_escaping: only `"`, `\`, and U+0000-U+001F are escaped.
    let mut object = Object::new();
    object.insert(
        Key::new("message").unwrap(),
        Value::Str("caf\u{e9} \"q\" a/b\nline\u{1f}end \u{1f600} \u{2028}x\u{7f}".to_owned()),
    );
    object.insert(Key::new("code").unwrap(), Value::Str("x".to_owned()));
    let bytes = to_canonical(&Value::Object(object));
    assert_eq!(
        Digest::of(&bytes).to_hex(),
        "876d71ad278c88d66d357c90c8c0c799fa780bb6010b4d7c42b6b5ee53249de4"
    );
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "7b22636f6465223a2278222c226d657373616765223a22636166c3a9205c22715c2220612f625c6e6c696e655c7530303166656e6420f09f988020e280a8787f227d"
    );
}

#[test]
#[ignore = "pending E5-1"]
fn writer_escapes() {
    assert_eq!(
        canonical(r#""\u0000\u0008\u000C\n\r\t\u001F\u0020\u007f\/\u00E9""#),
        "\"\\u0000\\b\\f\\n\\r\\t\\u001f \u{7f}/\u{e9}\""
    );
    assert_eq!(
        canonical(r#""\b\f\n\r\t\"\\\/""#),
        "\"\\b\\f\\n\\r\\t\\\"\\\\/\""
    );
    assert_eq!(canonical(r#""\ud83d\ude00""#), "\"\u{1f600}\"");
    assert_eq!(canonical("\"\u{2028}\u{2029}\""), "\"\u{2028}\u{2029}\"");
}

#[test]
#[ignore = "pending E5-1"]
fn accepts_and_canonicalizes() {
    assert_eq!(
        canonical(" { \"b\" : [ 1 , true , null ] ,\n\t\"a\":{\"z\":\"\",\"y\":0}}\r\n"),
        r#"{"a":{"y":0,"z":""},"b":[1,true,null]}"#
    );
    assert_eq!(canonical("9007199254740991"), "9007199254740991");
    assert_eq!(canonical("[]"), "[]");
    assert_eq!(canonical("{}"), "{}");
    assert_eq!(canonical("false"), "false");
    assert_eq!(
        canonical(r#"{"a_1":1,"a":2,"a0":3,"b":4}"#),
        r#"{"a":2,"a0":3,"a_1":1,"b":4}"#
    );
}

#[test]
#[ignore = "pending E5-1"]
fn rejects() {
    use ParseErrorKind::*;
    let cases: &[(&str, ParseErrorKind)] = &[
        ("150.0", Float),
        ("1e3", Float),
        ("1E+3", Float),
        ("-1.5", Float),
        ("0.5", Float),
        ("-1", IntegerRange),
        ("-0", IntegerRange),
        ("9007199254740992", IntegerRange),
        ("123456789012345678901234567890", IntegerRange),
        ("01", Syntax),
        ("-", Syntax),
        ("1.", Syntax),
        ("1e", Syntax),
        (".5", Syntax),
        ("+1", Syntax),
        ("NaN", Syntax),
        ("Infinity", Syntax),
        ("", Syntax),
        ("tr", Syntax),
        ("truex", TrailingData),
        ("nul", Syntax),
        ("[1,]", Syntax),
        ("[1 2]", Syntax),
        ("{\"a\":1,}", Syntax),
        ("{\"a\" 1}", Syntax),
        ("{a:1}", Syntax),
        ("\"abc", Syntax),
        ("\"a\u{1}b\"", Syntax),
        ("\"a\nb\"", Syntax),
        (r#""\x""#, Syntax),
        (r#""\u12""#, Syntax),
        (r#""\u12G4""#, Syntax),
        ("'a'", Syntax),
        (r#"{"a":1,"a":2}"#, DuplicateKey),
        (r#"{"a":{"b":1,"b":1}}"#, DuplicateKey),
        (r#"{"A":1}"#, InvalidKey),
        (r#"{"_a":1}"#, InvalidKey),
        (r#"{"1a":1}"#, InvalidKey),
        (r#"{"a-b":1}"#, InvalidKey),
        (r#"{"":1}"#, InvalidKey),
        (r#"{"caf\u00e9":1}"#, InvalidKey),
        (r#""\ud800""#, LoneSurrogate),
        (r#""\udc00""#, LoneSurrogate),
        (r#""\ud800\u0041""#, LoneSurrogate),
        (r#""\ud800x""#, LoneSurrogate),
        (r#""\ude00\ud83d""#, LoneSurrogate),
        ("{} x", TrailingData),
        ("1 1", TrailingData),
        ("{}{}", TrailingData),
    ];
    for (input, expected) in cases {
        assert_eq!(kind(input), Some(*expected), "{input:?}");
    }
    let long_key = format!(r#"{{"{}":1}}"#, "a".repeat(65));
    assert_eq!(kind(&long_key), Some(InvalidKey));
    assert_eq!(parse(b"\"\xff\"").unwrap_err().kind, InvalidUtf8);
    assert_eq!(parse(b"\"\xc3\"").unwrap_err().kind, InvalidUtf8);
    assert!(parse(format!(r#"{{"{}":1}}"#, "a".repeat(64)).as_bytes()).is_ok());
}

#[test]
#[ignore = "pending E5-1"]
fn error_offsets_and_codes() {
    let err = parse(br#"{"a":1,"a":2}"#).unwrap_err();
    assert_eq!((err.kind, err.offset), (ParseErrorKind::DuplicateKey, 7));
    assert_eq!(parse(b"[1, 2.5]").unwrap_err().offset, 4);
    assert_eq!(parse(b"  x").unwrap_err().offset, 2);
    use ParseErrorKind::*;
    let codes: Vec<&str> = [
        Syntax,
        Float,
        IntegerRange,
        DuplicateKey,
        InvalidKey,
        LoneSurrogate,
        InvalidUtf8,
        TooDeep,
        TrailingData,
    ]
    .iter()
    .map(|k| k.code())
    .collect();
    assert_eq!(
        codes,
        [
            "syntax",
            "float",
            "integer_range",
            "duplicate_key",
            "invalid_key",
            "lone_surrogate",
            "invalid_utf8",
            "too_deep",
            "trailing_data"
        ]
    );
}

#[test]
#[ignore = "pending E5-1"]
fn nesting_limit() {
    let nested = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(parse(nested(MAX_DEPTH).as_bytes()).is_ok());
    assert_eq!(
        parse(nested(MAX_DEPTH + 1).as_bytes()).unwrap_err().kind,
        ParseErrorKind::TooDeep
    );
    let objects = format!(
        "{}1{}",
        "{\"a\":".repeat(MAX_DEPTH + 1),
        "}".repeat(MAX_DEPTH + 1)
    );
    assert_eq!(
        parse(objects.as_bytes()).unwrap_err().kind,
        ParseErrorKind::TooDeep
    );
    // Depth is released on the way out, so siblings do not accumulate.
    let siblings = format!("[{}]", vec![nested(MAX_DEPTH - 1); 3].join(","));
    assert!(parse(siblings.as_bytes()).is_ok());
}

#[test]
#[ignore = "pending E5-1"]
fn keys_and_ints() {
    for ok in ["a", "z9", "a_b", "abc_123", &"x".repeat(64)] {
        assert_eq!(Key::new(ok).unwrap().as_str(), ok);
    }
    for bad in ["", "A", "_", "9a", "a-b", "a b", "\u{e9}", &"x".repeat(65)] {
        assert!(Key::new(bad).is_err(), "{bad:?}");
    }
    assert_eq!(Key::new("seq").unwrap().to_string(), "seq");
    assert_eq!(Int::new(MAX_INT).map(Int::get), Some(9_007_199_254_740_991));
    assert_eq!(Int::new(MAX_INT + 1), None);
    assert_eq!(Int::new(0).map(Int::get), Some(0));
}

#[test]
#[ignore = "pending E5-1"]
fn accessors() {
    let v = parse(br#"{"a":"x","b":7,"c":[null],"d":{}}"#).unwrap();
    assert_eq!(v.get("a").and_then(Value::as_str), Some("x"));
    assert_eq!(v.get("b").and_then(Value::as_int), Some(7));
    assert_eq!(
        v.get("c").and_then(Value::as_array),
        Some(&[Value::Null][..])
    );
    assert_eq!(v.get("d").and_then(Value::as_object), Some(&Object::new()));
    assert_eq!(v.get("zz"), None);
    assert_eq!(v.get("a").and_then(Value::as_int), None);
    assert_eq!(v.get("b").and_then(Value::as_str), None);
    assert_eq!(v.get("a").and_then(Value::as_array), None);
    assert_eq!(v.get("a").and_then(Value::as_object), None);
    assert_eq!(Value::Null.get("a"), None);
}

#[test]
#[ignore = "pending E5-1"]
fn sha256_and_digests() {
    assert_eq!(
        Digest::of(b"abc").to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        Digest::of(b"").to_string(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(Digest::of_parts(&[b"a", b"", b"bc"]), Digest::of(b"abc"));
    assert_eq!(Digest::ZERO.to_hex(), "0".repeat(64));
    let d = Digest::of(b"x");
    assert_eq!(Digest::from_hex(&d.to_hex()), Some(d));
    assert_eq!(Digest::from_bytes(*d.as_bytes()), d);
    let hex = d.to_hex();
    assert_eq!(Digest::from_hex(&hex.to_uppercase()), None);
    assert_eq!(Digest::from_hex(&hex[..63]), None);
    assert_eq!(Digest::from_hex(&format!("{hex}0")), None);
    assert_eq!(Digest::from_hex(&format!("g{}", &hex[1..])), None);
    assert_eq!(
        Digest::from_hex(&"f".repeat(64)).map(|d| *d.as_bytes()),
        Some([0xff; 32])
    );
    assert_eq!(
        Digest::from_hex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .map(|d| d.as_bytes()[..8].to_vec()),
        Some(vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
    );
}

// ------------------------------------------------------------------ properties

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        "[\\x00-\\x1f\"\\\\/\\x7f\u{2028}\u{2029} a\u{e9}\u{1f600}]{0,12}",
    ]
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        prop_oneof![0..=MAX_INT, Just(MAX_INT), 0u64..1000]
            .prop_map(|n| Value::Int(Int::new(n).unwrap())),
        text().prop_map(Value::Str),
    ];
    leaf.prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..6).prop_map(Value::Array),
            btree_map("[a-z][a-z0-9_]{0,10}", inner, 0..6).prop_map(|m| {
                Value::Object(
                    m.into_iter()
                        .map(|(k, v)| (Key::new(&k).unwrap(), v))
                        .collect(),
                )
            }),
        ]
    })
}

fn to_serde(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::from(i.get()),
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Array(a) => serde_json::Value::Array(a.iter().map(to_serde).collect()),
        Value::Object(o) => serde_json::Value::Object(
            o.iter()
                .map(|(k, v)| (k.as_str().to_owned(), to_serde(v)))
                .collect(),
        ),
    }
}

/// Members in reverse order, whitespace everywhere, every non-ASCII or special character escaped.
fn scrambled(v: &Value, out: &mut String) {
    const WS: &str = " \n\t\r";
    out.push_str(WS);
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.get().to_string()),
        Value::Str(s) => scrambled_str(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                scrambled(item, out);
            }
            out.push_str(WS);
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, item)) in o.iter().rev().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(WS);
                scrambled_str(k.as_str(), out);
                out.push_str(WS);
                out.push(':');
                scrambled(item, out);
            }
            out.push_str(WS);
            out.push('}');
        }
    }
    out.push_str(WS);
}

fn scrambled_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == ' ' {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{unit:04X}"));
            }
        }
    }
    out.push('"');
}

proptest! {
    #[test]
    #[ignore = "pending E5-1"]
    fn matches_independent_rfc8785_implementation(v in value()) {
        let expected = serde_json_canonicalizer::to_vec(&to_serde(&v)).unwrap();
        prop_assert_eq!(to_canonical(&v), expected);
    }

    #[test]
    #[ignore = "pending E5-1"]
    fn parse_inverts_write(v in value()) {
        prop_assert_eq!(parse(&to_canonical(&v)).unwrap(), v);
    }

    #[test]
    #[ignore = "pending E5-1"]
    fn whitespace_member_order_and_escapes_do_not_change_the_canonical_form(v in value()) {
        let mut text = String::new();
        scrambled(&v, &mut text);
        let reparsed = parse(text.as_bytes()).unwrap();
        prop_assert_eq!(to_canonical(&reparsed), to_canonical(&v));
    }

    #[test]
    fn arbitrary_bytes_never_panic_and_canonicalize_stably(bytes in vec(any::<u8>(), 0..64)) {
        if let Ok(v) = parse(&bytes) {
            let once = to_canonical(&v);
            prop_assert_eq!(to_canonical(&parse(&once).unwrap()), once);
        }
    }

    #[test]
    fn json_like_text_never_panics(s in "[\\[\\]{}\":,0-9.eE+\\-a-z\\\\ ]{0,40}") {
        if let Ok(v) = parse(s.as_bytes()) {
            prop_assert_eq!(parse(&to_canonical(&v)).unwrap(), v);
        }
    }
}
