//! The endpoint is configuration, checked before anything is sent, and an unreachable store fails
//! the check closed: no answer is never evidence (DEC-973).

use jiff::Timestamp;
use mandate_objectstore::check::{CheckError, check_s3};
use mandate_objectstore::s3::{ConfigError, Endpoint, S3, S3Error, is_plain_key};
use mandate_objectstore::sigv4::Credentials;
use secrecy::SecretString;

#[test]
fn https_anywhere_and_plain_http_only_on_loopback() {
    for url in [
        "https://s3.us-west-002.backblazeb2.com",
        "https://abc123.r2.cloudflarestorage.com:443",
        "http://127.0.0.1:9000",
        "http://localhost:9000/",
        "http://[::1]:9000",
    ] {
        assert!(
            Endpoint::new(url, "us-east-1", "journal-cold").is_ok(),
            "{url}"
        );
    }
    assert_eq!(
        Endpoint::new("http://minio.internal:9000", "us-east-1", "journal-cold"),
        Err(ConfigError::PlainHttp)
    );
    assert_eq!(
        Endpoint::new("http://10.0.0.5:9000", "us-east-1", "journal-cold"),
        Err(ConfigError::PlainHttp)
    );
    for url in [
        "not a url",
        "ftp://127.0.0.1",
        "https://host/bucket",
        "https://host/?x=1",
        "https://user:pass@host",
        "https://host/#frag",
    ] {
        assert_eq!(
            Endpoint::new(url, "us-east-1", "journal-cold"),
            Err(ConfigError::Url),
            "{url}"
        );
    }
}

#[test]
fn bucket_and_region_names_are_checked() {
    let ok = "https://s3.example.com";
    for bucket in [
        "ab",
        "Journal",
        "journal_cold",
        "journal/cold",
        &"a".repeat(64),
    ] {
        assert_eq!(
            Endpoint::new(ok, "us-east-1", bucket),
            Err(ConfigError::Name),
            "{bucket}"
        );
    }
    assert!(Endpoint::new(ok, "us-east-1", "abc").is_ok());
    assert!(Endpoint::new(ok, "us-east-1", &"a".repeat(63)).is_ok());
    assert!(Endpoint::new(ok, "auto", "journal.cold-1").is_ok());
    for region in ["", "US-EAST-1", "us east"] {
        assert_eq!(
            Endpoint::new(ok, region, "journal-cold"),
            Err(ConfigError::Name),
            "{region:?}"
        );
    }
    let endpoint = Endpoint::new("https://s3.example.com:8443", "auto", "journal-cold");
    assert_eq!(endpoint.as_ref().map(Endpoint::bucket), Ok("journal-cold"));
    assert_eq!(
        endpoint.as_ref().map(Endpoint::base),
        Ok("https://s3.example.com:8443/")
    );
}

#[tokio::test]
async fn an_unreachable_store_fails_the_check_closed() {
    let endpoint = match Endpoint::new("http://127.0.0.1:1", "us-east-1", "journal-cold") {
        Ok(endpoint) => endpoint,
        Err(error) => panic!("valid: {error}"),
    };
    let key = Credentials::new("id".to_owned(), SecretString::from("secret"), None);
    let store = match S3::new(endpoint, key, Timestamp::now) {
        Ok(store) => store,
        Err(error) => panic!("builds: {error}"),
    };
    assert_eq!(
        check_s3(&store, "cold/").await.map(|_| ()),
        Err(CheckError::Store(S3Error::Transport))
    );
}

#[test]
fn a_key_is_plain_and_relative() {
    for good in [
        "a",
        "cold/segments/s-1.jsonl",
        "a.b/..c/c..",
        "x y",
        &"k".repeat(1024),
    ] {
        assert!(is_plain_key(good), "{good:?}");
    }
    for bad in [
        "",
        "/a",
        "a/",
        "a//b",
        ".",
        "..",
        "../x",
        "a/../b",
        "a/./b",
        "a/..",
        "a\nb",
        "a\u{7f}b",
        &"k".repeat(1025),
    ] {
        assert!(!is_plain_key(bad), "{bad:?}");
    }
}

#[tokio::test]
async fn a_traversing_key_or_prefix_is_refused_before_anything_is_sent() {
    let endpoint = match Endpoint::new("http://127.0.0.1:1", "us-east-1", "journal-cold") {
        Ok(endpoint) => endpoint,
        Err(error) => panic!("valid: {error}"),
    };
    let key = Credentials::new("id".to_owned(), SecretString::from("secret"), None);
    let store = match S3::new(endpoint, key, Timestamp::now) {
        Ok(store) => store,
        Err(error) => panic!("builds: {error}"),
    };
    for traversal in ["../other-bucket/x", "cold/../../x", "/x"] {
        assert_eq!(
            store.put_object(traversal, b"x", None).await,
            Err(S3Error::InvalidKey),
            "{traversal}"
        );
        assert_eq!(
            store.get_object(traversal, None).await,
            Err(S3Error::InvalidKey)
        );
        assert_eq!(
            store.delete_object(traversal).await,
            Err(S3Error::InvalidKey)
        );
    }
    for prefix in ["../", "cold", "", "cold//"] {
        assert_eq!(
            check_s3(&store, prefix).await.map(|_| ()),
            Err(CheckError::Store(S3Error::InvalidKey)),
            "{prefix:?}"
        );
    }
}
