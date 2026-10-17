//! SigV4 against the worked examples of Amazon's S3 documentation ("Signature Calculations for the
//! Authorization Header", the four GET, PUT, lifecycle and list examples): an oracle computed by
//! Amazon, not by this crate.

use jiff::Timestamp;
use mandate_objectstore::sigv4::{
    Credentials, Request, Scope, amz_date, authorization, sha256_hex, uri_encode,
};
use secrecy::SecretString;

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const HOST: &str = "examplebucket.s3.amazonaws.com";
const AMZ_DATE: &str = "20130524T000000Z";

fn example_key() -> Credentials {
    Credentials::new(
        "AKIAIOSFODNN7EXAMPLE".to_owned(),
        SecretString::from("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
        None,
    )
}

fn scope() -> Scope<'static> {
    let at: Timestamp = match "2013-05-24T00:00:00Z".parse() {
        Ok(at) => at,
        Err(error) => panic!("the example's time parses: {error}"),
    };
    Scope {
        region: "us-east-1",
        service: "s3",
        at,
    }
}

fn expected(signed_headers: &str, signature: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
         SignedHeaders={signed_headers}, Signature={signature}"
    )
}

#[test]
fn get_object_with_a_range() {
    let headers = [
        ("Host", HOST),
        ("Range", "bytes=0-9"),
        ("x-amz-content-sha256", EMPTY_SHA256),
        ("x-amz-date", AMZ_DATE),
    ];
    let request = Request {
        method: "GET",
        path: "/test.txt",
        query: &[],
        headers: &headers,
        payload_sha256: EMPTY_SHA256,
    };
    assert_eq!(
        authorization(&example_key(), &request, &scope()),
        expected(
            "host;range;x-amz-content-sha256;x-amz-date",
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        )
    );
}

#[test]
fn put_object_with_an_encoded_key_and_a_body() {
    let body = sha256_hex(b"Welcome to Amazon S3.");
    assert_eq!(
        body,
        "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
    );
    let path = format!("/{}", uri_encode("test$file.text", true));
    assert_eq!(path, "/test%24file.text");
    let headers = [
        ("Date", "Fri, 24 May 2013 00:00:00 GMT"),
        ("Host", HOST),
        ("x-amz-date", AMZ_DATE),
        ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
        ("x-amz-content-sha256", &body),
    ];
    let request = Request {
        method: "PUT",
        path: &path,
        query: &[],
        headers: &headers,
        payload_sha256: &body,
    };
    assert_eq!(
        authorization(&example_key(), &request, &scope()),
        expected(
            "date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class",
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        )
    );
}

#[test]
fn a_subresource_with_no_value() {
    let headers = [
        ("Host", HOST),
        ("x-amz-date", AMZ_DATE),
        ("x-amz-content-sha256", EMPTY_SHA256),
    ];
    let request = Request {
        method: "GET",
        path: "/",
        query: &[("lifecycle", "")],
        headers: &headers,
        payload_sha256: EMPTY_SHA256,
    };
    assert_eq!(
        authorization(&example_key(), &request, &scope()),
        expected(
            "host;x-amz-content-sha256;x-amz-date",
            "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"
        )
    );
}

#[test]
fn query_parameters_are_sorted() {
    let headers = [
        ("Host", HOST),
        ("x-amz-date", AMZ_DATE),
        ("x-amz-content-sha256", EMPTY_SHA256),
    ];
    let request = Request {
        method: "GET",
        path: "/",
        query: &[("prefix", "J"), ("max-keys", "2")],
        headers: &headers,
        payload_sha256: EMPTY_SHA256,
    };
    assert_eq!(
        authorization(&example_key(), &request, &scope()),
        expected(
            "host;x-amz-content-sha256;x-amz-date",
            "34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
        )
    );
}

#[test]
fn the_amz_date_form() {
    assert_eq!(amz_date(scope().at), AMZ_DATE);
    let later: Timestamp = match "2026-10-17T04:05:06.789Z".parse() {
        Ok(at) => at,
        Err(error) => panic!("parses: {error}"),
    };
    assert_eq!(amz_date(later), "20261017T040506Z");
}

#[test]
fn uri_encoding_keeps_only_the_unreserved_set() {
    assert_eq!(uri_encode("AZaz09-._~", false), "AZaz09-._~");
    assert_eq!(uri_encode("a b/c+d=é", false), "a%20b%2Fc%2Bd%3D%C3%A9");
    assert_eq!(uri_encode("a b/c", true), "a%20b/c");
    assert_eq!(uri_encode("", true), "");
}

#[test]
fn header_values_are_trimmed_and_their_inner_spaces_collapsed() {
    let spaced = [
        ("Host", HOST),
        ("Range", "  bytes=0-9  "),
        ("x-amz-content-sha256", EMPTY_SHA256),
        ("X-Amz-Date", AMZ_DATE),
    ];
    let request = Request {
        method: "GET",
        path: "/test.txt",
        query: &[],
        headers: &spaced,
        payload_sha256: EMPTY_SHA256,
    };
    assert_eq!(
        authorization(&example_key(), &request, &scope()),
        expected(
            "host;range;x-amz-content-sha256;x-amz-date",
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        )
    );
    let inner = [
        ("Host", HOST),
        ("Range", "bytes=0-9"),
        ("x-amz-content-sha256", EMPTY_SHA256),
        ("x-amz-date", AMZ_DATE),
        ("x-amz-meta-note", "a   b"),
    ];
    let collapsed = [
        ("Host", HOST),
        ("Range", "bytes=0-9"),
        ("x-amz-content-sha256", EMPTY_SHA256),
        ("x-amz-date", AMZ_DATE),
        ("x-amz-meta-note", "a b"),
    ];
    let sign = |headers: &[(&str, &str)]| {
        authorization(
            &example_key(),
            &Request {
                method: "GET",
                path: "/test.txt",
                query: &[],
                headers,
                payload_sha256: EMPTY_SHA256,
            },
            &scope(),
        )
    };
    assert_eq!(sign(&inner), sign(&collapsed));
    assert_ne!(sign(&inner), sign(&spaced));
}

#[test]
fn a_session_token_is_kept_and_never_printed() {
    let key = Credentials::new(
        "ASIATEMP".to_owned(),
        SecretString::from("secret-value"),
        Some(SecretString::from("token-value")),
    );
    assert_eq!(key.session_token(), Some("token-value"));
    assert_eq!(example_key().session_token(), None);
    let printed = format!("{key:?}");
    assert!(
        !printed.contains("secret-value")
            && !printed.contains("token-value")
            && !printed.contains("ASIATEMP")
    );
}

#[test]
fn a_key_is_read_from_its_two_mounted_files() {
    let dir = std::env::temp_dir().join(format!("mandate-objectstore-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("dir: {error}"));
    let id = dir.join("access-key-id");
    let secret = dir.join("secret-access-key");
    std::fs::write(&id, "AKIAIOSFODNN7EXAMPLE\n").unwrap_or_else(|error| panic!("id: {error}"));
    std::fs::write(&secret, "  wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n")
        .unwrap_or_else(|error| panic!("secret: {error}"));
    let key =
        Credentials::from_files(&id, &secret).unwrap_or_else(|error| panic!("reads: {error}"));
    assert_eq!(key.session_token(), None);
    let headers = [
        ("Host", HOST),
        ("Range", "bytes=0-9"),
        ("x-amz-content-sha256", EMPTY_SHA256),
        ("x-amz-date", AMZ_DATE),
    ];
    let request = Request {
        method: "GET",
        path: "/test.txt",
        query: &[],
        headers: &headers,
        payload_sha256: EMPTY_SHA256,
    };
    assert_eq!(
        authorization(&key, &request, &scope()),
        authorization(&example_key(), &request, &scope())
    );
    assert!(Credentials::from_files(&dir.join("missing"), &secret).is_err());
    assert!(Credentials::from_files(&id, &dir.join("missing")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
