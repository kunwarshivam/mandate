//! Published test vectors and the fixtures that hold them, for `src/tests.rs` only (DEC-794). The
//! keys, salts and messages are RFC 8291 §5, Appendix A, and RFC 8188 §3.1's examples, never used
//! outside tests. CodeQL's `paths-ignore` names exactly this directory, and
//! `no_product_code_reaches_the_rfc_vectors` pins that nothing but the tests module includes it.

use crate::{SecureRandom, VapidSigner, WebPushError};
use aes_gcm::Aes128Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use base64ct::{Base64UrlUnpadded, Encoding};
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

pub fn b64(text: &str) -> Vec<u8> {
    Base64UrlUnpadded::decode_vec(text).unwrap_or_default()
}

/// Hands out the bytes it was given, in order, and fails once they run out.
pub struct Scripted(Vec<u8>);

impl SecureRandom for Scripted {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), WebPushError> {
        if self.0.len() < out.len() {
            return Err(WebPushError::Random);
        }
        let rest = self.0.split_off(out.len());
        out.copy_from_slice(&self.0);
        self.0 = rest;
        Ok(())
    }
}

/// A test key only: RFC 8291 §5's sender key, published in the RFC.
pub struct TestSigner(SigningKey);

impl TestSigner {
    pub fn new() -> Self {
        Self(SigningKey::from_slice(&b64(AS_PRIVATE)).unwrap_or_else(|_| unreachable!()))
    }
}

impl VapidSigner for TestSigner {
    fn public_key(&self) -> [u8; 65] {
        let point = self.0.verifying_key().to_sec1_point(false);
        point.as_bytes().try_into().unwrap_or([0; 65])
    }
    fn sign_es256(&self, message: &[u8]) -> Result<[u8; 64], WebPushError> {
        let signature: Signature = self.0.sign(message);
        Ok(signature.to_bytes().into())
    }
}

pub const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
pub const UA_PUBLIC: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
pub const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";

pub fn rfc_random() -> Scripted {
    Scripted([b64(AS_PRIVATE), b64(SALT)].concat())
}

/// The receiver's side of RFC 8291 §3 and RFC 8188 §2, written apart from the sender's.
pub fn open(body: &[u8]) -> Vec<u8> {
    let (salt, rest) = body.split_at(16);
    let (_, rest) = rest.split_at(5);
    let (as_public, sealed) = rest.split_at(65);
    let ua = SecretKey::from_slice(&b64(UA_PRIVATE)).unwrap_or_else(|_| unreachable!());
    let sender = PublicKey::from_sec1_bytes(as_public).unwrap_or_else(|_| unreachable!());
    let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), sender.as_affine());
    let mut ikm = [0u8; 32];
    let info = [b"WebPush: info\0".as_slice(), &b64(UA_PUBLIC), as_public].concat();
    let _ =
        Hkdf::<Sha256>::new(Some(&b64(AUTH)), shared.raw_secret_bytes()).expand(&info, &mut ikm);
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    let _ = hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek);
    let _ = hk.expand(b"Content-Encoding: nonce\0", &mut nonce);
    Aes128Gcm::new_from_slice(&cek)
        .unwrap_or_else(|_| unreachable!())
        .decrypt(&nonce.into(), sealed)
        .unwrap_or_default()
}

/// A key draw of zeros, which is not a valid P-256 scalar.
pub fn zero_random() -> Scripted {
    Scripted(vec![0; 40])
}

/// A key draw from `seed`, then a fixed salt.
pub fn seeded_random(seed: [u8; 32]) -> Scripted {
    Scripted([seed.to_vec(), vec![9; 16]].concat())
}

/// An auth secret one octet short.
pub fn short_auth() -> [u8; 15] {
    [0; 15]
}

/// RFC 8291 §5's message, the sender's key and salt above encrypting "When I grow up, I want to be
/// a watermelon" to the receiver.
pub const RFC_8291_MESSAGE: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
                                    mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
                                    pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

/// RFC 8188 §3.1's input keying material and its encryption of "I am the walrus".
pub const RFC_8188_IKM: &str = "yqdlZ-tYemfogSmv7Ws5PQ";
pub const RFC_8188_MESSAGE: &str =
    "I1BsxtFttlv3u_Oo94xnmwAAEAAA-NAVub2qFgBEuQKRapoZu-IxkIva3MEB1PD-ly8Thjg";
