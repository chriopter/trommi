//! EHBP, the Encrypted HTTP Body Protocol (docs.tinfoil.sh/resources/ehbp), the client's half: a request body
//! sealed to an enclave's HPKE key, and the answer opened. Headers stay readable on the way (the hub relays
//! them); the bodies are for the enclave and this device alone.
//!
//! - **Request**: HPKE base mode (RFC 9180), DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, AES-256-GCM, `info` =
//!   "ehbp request". The body is one chunk: a 4-byte big-endian length, then `Seal("", body)`. The encapsulated
//!   key goes as lowercase hex in `Ehbp-Encapsulated-Key`.
//! - **Response**: `secret = Export("ehbp response", 32)` of the request's context; `prk = Extract(enc ‖ nonce,
//!   secret)` with the 32-byte `Ehbp-Response-Nonce`; `key = Expand(prk, "key", 32)`, `base = Expand(prk,
//!   "nonce", 12)`. The body is chunks as above; chunk `i` is AES-256-GCM under `key` with the nonce `base` XOR
//!   `i` (big-endian, in its last 8 bytes) and no AAD. An empty chunk is skipped; a cut chunk fails.
//!
//! Checked against the reference JavaScript client in `core/client/src/dictation/ehbp-vectors.json`.

use hpke_rs::{Hpke, HpkePublicKey, Mode};
use hpke_rs_crypto::types::{AeadAlgorithm, KdfAlgorithm, KemAlgorithm};
use hpke_rs_crypto::HpkeCrypto;
use hpke_rs_rust_crypto::HpkeRustCrypto;

pub const REQUEST_INFO: &[u8] = b"ehbp request";
pub const EXPORT_LABEL: &[u8] = b"ehbp response";
pub const ENCAPSULATED_KEY_HEADER: &str = "ehbp-encapsulated-key";
pub const RESPONSE_NONCE_HEADER: &str = "ehbp-response-nonce";
/// The most bytes one chunk of an answer may hold (a transcript is far smaller).
const MAX_CHUNK: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EhbpError {
    /// The enclave's key is no X25519 public key.
    BadKey,
    /// HPKE failed (no randomness).
    Seal,
    /// The answer's nonce, framing or a chunk's tag is wrong.
    Open,
}

fn hpke() -> Hpke<HpkeRustCrypto> {
    Hpke::new(Mode::Base, KemAlgorithm::DhKem25519, KdfAlgorithm::HkdfSha256, AeadAlgorithm::Aes256Gcm)
}

/// A request body sealed to the enclave, and what opens its answer.
pub struct Sealed {
    /// The body to send: one length-prefixed chunk.
    pub body: Vec<u8>,
    /// `Ehbp-Encapsulated-Key`, lowercase hex.
    pub encapsulated_key: String,
    pub opener: Opener,
}

impl std::fmt::Debug for Sealed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sealed({} bytes)", self.body.len())
    }
}

/// What opens the answer to one sealed request: the exported secret and the encapsulated key. Never logged.
#[derive(Clone)]
pub struct Opener {
    secret: [u8; 32],
    enc: [u8; 32],
}

impl std::fmt::Debug for Opener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Opener(<redacted>)")
    }
}

impl Drop for Opener {
    fn drop(&mut self) {
        self.secret = [0; 32];
    }
}

/// Seals `plain` to the enclave's X25519 public key (32 bytes, as its attestation names it).
pub fn seal(public_key: &[u8; 32], plain: &[u8]) -> Result<Sealed, EhbpError> {
    let mut hpke = hpke();
    let key = HpkePublicKey::from(&public_key[..]);
    let (enc, mut context) = hpke.setup_sender(&key, REQUEST_INFO, None, None, None).map_err(|_| EhbpError::BadKey)?;
    let enc: [u8; 32] = enc.as_slice().try_into().map_err(|_| EhbpError::Seal)?;
    let sealed = context.seal(&[], plain).map_err(|_| EhbpError::Seal)?;
    let secret: [u8; 32] = context
        .export(EXPORT_LABEL, 32)
        .map_err(|_| EhbpError::Seal)?
        .as_slice()
        .try_into()
        .map_err(|_| EhbpError::Seal)?;
    let length = u32::try_from(sealed.len()).map_err(|_| EhbpError::Seal)?;
    let mut body = Vec::with_capacity(4 + sealed.len());
    body.extend_from_slice(&length.to_be_bytes());
    body.extend_from_slice(&sealed);
    Ok(Sealed { body, encapsulated_key: hex(&enc), opener: Opener { secret, enc } })
}

impl Opener {
    #[cfg(test)]
    pub(crate) fn from_parts(secret: [u8; 32], enc: [u8; 32]) -> Self {
        Opener { secret, enc }
    }

    /// Opens an answer: `nonce` is its `Ehbp-Response-Nonce` (64 hex digits), `body` its bytes.
    pub fn open(&self, nonce: &str, body: &[u8]) -> Result<Vec<u8>, EhbpError> {
        let nonce = unhex::<32>(nonce).ok_or(EhbpError::Open)?;
        let salt = [&self.enc[..], &nonce[..]].concat();
        let kdf = KdfAlgorithm::HkdfSha256;
        let prk = HpkeRustCrypto::kdf_extract(kdf, &salt, &self.secret).map_err(|_| EhbpError::Open)?;
        let key = HpkeRustCrypto::kdf_expand(kdf, &prk, b"key", 32).map_err(|_| EhbpError::Open)?;
        let base = HpkeRustCrypto::kdf_expand(kdf, &prk, b"nonce", 12).map_err(|_| EhbpError::Open)?;
        let mut out = Vec::new();
        let mut rest = body;
        let mut seq: u64 = 0;
        while !rest.is_empty() {
            let (head, tail) = rest.split_at_checked(4).ok_or(EhbpError::Open)?;
            let length = u32::from_be_bytes(head.try_into().map_err(|_| EhbpError::Open)?) as usize;
            if length > MAX_CHUNK {
                return Err(EhbpError::Open);
            }
            let (chunk, tail) = tail.split_at_checked(length).ok_or(EhbpError::Open)?;
            rest = tail;
            if length == 0 {
                continue;
            }
            let mut chunk_nonce = [0u8; 12];
            chunk_nonce.copy_from_slice(&base);
            for (i, b) in seq.to_be_bytes().iter().enumerate() {
                chunk_nonce[4 + i] ^= b;
            }
            let plain = HpkeRustCrypto::aead_open(AeadAlgorithm::Aes256Gcm, &key, &chunk_nonce, &[], chunk)
                .map_err(|_| EhbpError::Open)?;
            out.extend_from_slice(&plain);
            seq += 1;
        }
        Ok(out)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpke_rs::HpkePrivateKey;

    fn unhex_vec(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!("ehbp-vectors.json")).unwrap()
    }

    /// The reference client's sealed request opens with the enclave's private key here: the same suite, info and
    /// framing.
    #[test]
    fn opens_what_the_reference_client_sealed() {
        let v = &vectors()["request"];
        let body = unhex_vec(v["body"].as_str().unwrap());
        let length = u32::from_be_bytes(body[..4].try_into().unwrap()) as usize;
        assert_eq!(length, body.len() - 4);
        let private = HpkePrivateKey::from(unhex_vec(v["private_key"].as_str().unwrap()));
        let context = hpke().setup_receiver(&unhex_vec(v["enc"].as_str().unwrap()), &private, REQUEST_INFO, None, None, None);
        let plain = context.unwrap().open(&[], &body[4..]).unwrap();
        assert_eq!(hex(&plain), v["plain"].as_str().unwrap());
    }

    /// A request sealed here opens at the enclave, and the answer the enclave seals with the exported secret
    /// opens here.
    #[test]
    fn seals_for_the_enclave_and_opens_its_answer() {
        let mut keys = hpke();
        let pair = keys.generate_key_pair().unwrap();
        let public: [u8; 32] = pair.public_key().as_slice().try_into().unwrap();
        let plain = b"a recording".to_vec();
        let sealed = seal(&public, &plain).unwrap();
        assert_eq!(sealed.encapsulated_key.len(), 64);
        let enc = unhex_vec(&sealed.encapsulated_key);
        let context = hpke().setup_receiver(&enc, pair.private_key(), REQUEST_INFO, None, None, None).unwrap();
        let mut enclave = context;
        assert_eq!(enclave.open(&[], &sealed.body[4..]).unwrap(), plain);
        // the enclave's side of the answer
        let secret = enclave.export(EXPORT_LABEL, 32).unwrap();
        let nonce = [9u8; 32];
        let salt = [&enc[..], &nonce[..]].concat();
        let prk = HpkeRustCrypto::kdf_extract(KdfAlgorithm::HkdfSha256, &salt, &secret).unwrap();
        let key = HpkeRustCrypto::kdf_expand(KdfAlgorithm::HkdfSha256, &prk, b"key", 32).unwrap();
        let base = HpkeRustCrypto::kdf_expand(KdfAlgorithm::HkdfSha256, &prk, b"nonce", 12).unwrap();
        let ct = HpkeRustCrypto::aead_seal(AeadAlgorithm::Aes256Gcm, &key, &base, &[], b"{\"text\":\"ok\"}").unwrap();
        let mut answer = (ct.len() as u32).to_be_bytes().to_vec();
        answer.extend_from_slice(&ct);
        assert_eq!(sealed.opener.open(&hex(&nonce), &answer).unwrap(), b"{\"text\":\"ok\"}");
        // another nonce, a flipped bit, a cut body: refused
        assert_eq!(sealed.opener.open(&hex(&[8u8; 32]), &answer), Err(EhbpError::Open));
        let mut flipped = answer.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert_eq!(sealed.opener.open(&hex(&nonce), &flipped), Err(EhbpError::Open));
        assert_eq!(sealed.opener.open(&hex(&nonce), &answer[..answer.len() - 1]), Err(EhbpError::Open));
        assert_eq!(sealed.opener.open("zz", &answer), Err(EhbpError::Open));
    }

    /// The reference client's chunked answer, two chunks, opens here.
    #[test]
    fn opens_the_reference_answer() {
        let v = &vectors()["response"];
        let secret: [u8; 32] = unhex_vec(v["exported_secret"].as_str().unwrap()).try_into().unwrap();
        let enc: [u8; 32] = unhex_vec(v["request_enc"].as_str().unwrap()).try_into().unwrap();
        let opener = Opener::from_parts(secret, enc);
        let plain = opener.open(v["nonce"].as_str().unwrap(), &unhex_vec(v["body"].as_str().unwrap())).unwrap();
        assert_eq!(String::from_utf8(plain).unwrap(), v["plain"].as_str().unwrap());
    }

    #[test]
    fn two_seals_differ() {
        let pair = hpke().generate_key_pair().unwrap();
        let public: [u8; 32] = pair.public_key().as_slice().try_into().unwrap();
        let a = seal(&public, b"x").unwrap();
        let b = seal(&public, b"x").unwrap();
        assert_ne!(a.encapsulated_key, b.encapsulated_key);
        assert_ne!(a.body, b.body);
    }
}
