use super::*;
use hpke_rs::{Hpke, HpkeKeyPair, Mode};
use hpke_rs_crypto::types::{AeadAlgorithm, KdfAlgorithm, KemAlgorithm};
use hpke_rs_crypto::HpkeCrypto;
use hpke_rs_rust_crypto::HpkeRustCrypto;

fn hpke() -> Hpke<HpkeRustCrypto> {
    Hpke::new(Mode::Base, KemAlgorithm::DhKem25519, KdfAlgorithm::HkdfSha256, AeadAlgorithm::Aes256Gcm)
}

/// A stand-in enclave: its key pair, and how it opens a request and seals its answer.
struct Enclave(HpkeKeyPair);

impl Enclave {
    fn new() -> Self {
        Enclave(hpke().generate_key_pair().unwrap())
    }
    fn attested(&self) -> Attested {
        Attested { hpke_key: self.0.public_key().as_slice().try_into().unwrap(), domain: "router.inf6.tinfoil.sh".into() }
    }
    /// Opens the sealed form, and answers `answer` sealed: (the form, the answer as the relay brings it).
    fn answer(&self, sealed: &crate::hub::stt::SttSealed, answer: &str) -> (Vec<u8>, Sent) {
        let enc = (0..64).step_by(2).map(|i| u8::from_str_radix(&sealed.encapsulated_key[i..i + 2], 16).unwrap()).collect::<Vec<_>>();
        let mut ctx = hpke().setup_receiver(&enc, self.0.private_key(), ehbp::REQUEST_INFO, None, None, None).unwrap();
        let form = ctx.open(&[], &sealed.body[4..]).unwrap();
        let secret = ctx.export(ehbp::EXPORT_LABEL, 32).unwrap();
        let nonce = [5u8; 32];
        let prk = HpkeRustCrypto::kdf_extract(KdfAlgorithm::HkdfSha256, &[&enc[..], &nonce[..]].concat(), &secret).unwrap();
        let key = HpkeRustCrypto::kdf_expand(KdfAlgorithm::HkdfSha256, &prk, b"key", 32).unwrap();
        let base = HpkeRustCrypto::kdf_expand(KdfAlgorithm::HkdfSha256, &prk, b"nonce", 12).unwrap();
        let ct = HpkeRustCrypto::aead_seal(AeadAlgorithm::Aes256Gcm, &key, &base, &[], answer.as_bytes()).unwrap();
        let mut body = (ct.len() as u32).to_be_bytes().to_vec();
        body.extend_from_slice(&ct);
        (form, Sent::Answered { nonce: ehbp::hex(&nonce), body })
    }
}

fn wav() -> Vec<u8> {
    let mut w = b"RIFF....WAVEfmt ".to_vec();
    w.resize(4_000, 7);
    w
}

fn sends(out: &[Do]) -> Vec<&crate::hub::stt::SttSealed> {
    out.iter().filter_map(|d| if let Do::Send(s) = d { Some(s) } else { None }).collect()
}

fn state(d: &Dictation) -> String {
    d.view(true, 0).state
}

#[test]
fn a_dictation_from_tap_to_words() {
    let enclave = Enclave::new();
    let mut d = Dictation::default();
    d.set_locale("de-DE");
    assert_eq!(state(&d), "idle");
    assert_eq!(d.start(1_000, "s1"), vec![Do::Record { max_ms: MAX_MS }, Do::FetchBundle]);
    assert_eq!(state(&d), "starting");
    d.recording(1_100);
    let v = d.view(true, 0);
    assert_eq!((v.state.as_str(), v.since_ms), ("recording", 1_100));
    assert!(v.hint.is_some());
    // the bundle is checked while the person speaks
    assert_eq!(d.bundle(b"{}".to_vec()), vec![Do::Attest { bundle: b"{}".to_vec(), repo: CONFIG_REPO }]);
    assert!(d.attested(2_000, Some(enclave.attested())).is_empty(), "nothing to send before the recording");
    assert_eq!(d.stop(), vec![Do::Stop]);
    assert_eq!(state(&d), "working");
    let out = d.recorded(wav(), 4_000);
    let sent = sends(&out);
    assert_eq!(sent.len(), 1);
    let sealed = sent[0].clone();
    assert_eq!(sealed.enclave, "https://router.inf6.tinfoil.sh");
    assert_eq!(sealed.model, "voxtral-small-24b");
    assert!(!sealed.body.windows(4).any(|w| w == b"RIFF"), "the recording leaves sealed only");
    assert_eq!(state(&d), "working");
    let (form, answer) = enclave.answer(&sealed, r#"{"text":"  Hallo Welt.\u0007 "}"#);
    let form = String::from_utf8_lossy(&form);
    assert!(form.contains("name=\"model\"\r\n\r\nvoxtral-small-24b"));
    assert!(form.contains("name=\"language\"\r\n\r\nde"));
    assert!(form.contains("RIFF"));
    assert!(d.sent(answer).is_empty());
    let v = d.view(true, 0);
    assert_eq!(v.state, "done");
    let text = v.text.unwrap();
    assert_eq!(text.text, "Hallo Welt.");
    d.taken(text.id.parse().unwrap());
    assert_eq!(state(&d), "idle");
    // the checked enclave is used again within ten minutes: no second bundle
    assert_eq!(d.start(5_000, "s1"), vec![Do::Record { max_ms: MAX_MS }]);
    d.cancel();
    assert_eq!(d.start(5_000 + ATTESTED_FOR_MS + 1_000, "s1"), vec![Do::Record { max_ms: MAX_MS }, Do::FetchBundle]);
}

/// The rule of this module: when the attestation fails, nothing is sent, ever.
#[test]
fn a_failed_attestation_sends_nothing() {
    for before in [true, false] {
        let mut d = Dictation::default();
        let mut out = d.start(0, "s1");
        d.recording(10);
        out.extend(d.bundle(b"{}".to_vec()));
        if before {
            out.extend(d.attested(20, None));
        }
        out.extend(d.stop());
        out.extend(d.recorded(wav(), 3_000));
        if !before {
            out.extend(d.attested(4_000, None));
        }
        assert!(sends(&out).is_empty());
        let v = d.view(true, 0);
        assert_eq!(v.state, "failed");
        assert_eq!(v.problem.unwrap().key, "dictate.not_verified");
        // and the next dictation checks again
        d.dismiss();
        assert!(d.start(5_000, "s1").contains(&Do::FetchBundle));
    }
    // a verdict for another host than Tinfoil's is no verdict
    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(1);
    d.bundle(b"{}".to_vec());
    let mut wrong = Enclave::new().attested();
    wrong.domain = "evil.example".into();
    let mut out = d.attested(2, Some(wrong));
    out.extend(d.stop());
    out.extend(d.recorded(wav(), 3_000));
    assert!(sends(&out).is_empty());
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.not_verified");
    // no bundle (offline): nothing sent either
    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(1);
    let mut out = d.bundle_failed(Problem::Offline);
    out.extend(d.stop());
    out.extend(d.recorded(wav(), 3_000));
    assert!(sends(&out).is_empty());
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.offline");
}

#[test]
fn the_fallback_model_once_then_failed() {
    let enclave = Enclave::new();
    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(1);
    d.bundle(b"{}".to_vec());
    d.attested(2, Some(enclave.attested()));
    d.stop();
    let first = sends(&d.recorded(wav(), 2_000))[0].clone();
    let again = d.sent(Sent::Upstream);
    let second = sends(&again)[0].clone();
    assert_eq!(second.model, "whisper-large-v3-turbo");
    assert_ne!(first.encapsulated_key, second.encapsulated_key, "sealed anew");
    assert!(d.sent(Sent::Upstream).is_empty());
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.failed");
}

#[test]
fn answers_that_do_not_open_or_hold_no_words() {
    let enclave = Enclave::new();
    let run = |answer: &dyn Fn(&crate::hub::stt::SttSealed) -> Sent| {
        let mut d = Dictation::default();
        d.start(0, "s1");
        d.recording(1);
        d.bundle(b"{}".to_vec());
        d.attested(2, Some(enclave.attested()));
        d.stop();
        let sealed = sends(&d.recorded(wav(), 2_000))[0].clone();
        d.sent(answer(&sealed));
        d
    };
    let d = run(&|s| enclave.answer(s, r#"{"text":"   "}"#).1);
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.nothing_heard");
    let mut d = run(&|_| Sent::Answered { nonce: "00".repeat(32), body: vec![0, 0, 0, 2, 1, 2] });
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.failed");
    // (the enclave is checked again)
    assert!(d.start(10, "s1").contains(&Do::FetchBundle));
    let d = run(&|_| Sent::TooMany);
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.too_many");
    let d = run(&|_| Sent::Off);
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.off");
}

#[test]
fn short_long_refused_cancelled_and_the_clock() {
    let mut d = Dictation::default();
    d.start(0, "s1");
    d.mic_refused();
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.no_mic");

    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(0);
    d.stop();
    assert!(d.recorded(wav(), 200).is_empty());
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.too_short");

    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(0);
    d.stop();
    assert!(d.recorded(vec![0; MAX_AUDIO + 1], 200_000).is_empty());
    assert_eq!(d.view(true, 0).problem.unwrap().key, "dictate.too_long");

    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(100);
    assert_eq!(d.wake(), Some(100 + MAX_MS));
    assert!(d.tick(100 + MAX_MS - 1).is_empty());
    assert_eq!(d.tick(100 + MAX_MS), vec![Do::Stop]);
    assert_eq!(d.cancel(), vec![Do::Drop]);
    assert_eq!(state(&d), "idle");
    assert!(d.recorded(wav(), 5_000).is_empty(), "a recording after cancel is dropped");

    // a second tap while one runs starts nothing
    let mut d = Dictation::default();
    d.start(0, "s1");
    assert!(d.start(1, "s1").is_empty());
    // without the ability: off
    assert_eq!(d.view(false, 0).state, "off");
}

#[test]
fn locales() {
    let mut d = Dictation::default();
    for (locale, lang) in [("de-DE", Some("de")), ("en_US", Some("en")), ("ja-JP", None), ("", None)] {
        d.set_locale(locale);
        assert_eq!(d.language, lang, "{locale}");
    }
    let f = form(b"AUDIO", "m", None);
    assert!(!String::from_utf8_lossy(&f).contains("language"));
    assert!(f.ends_with(format!("\r\n--{BOUNDARY}--\r\n").as_bytes()));
}

/// A recording thrown away and a new one begun: what the first still delivers never enters the second.
#[test]
fn a_late_recording_does_not_enter_a_new_one() {
    let mut d = Dictation::default();
    d.start(0, "s1");
    d.recording(1);
    assert_eq!(d.cancel(), vec![Do::Drop]);
    d.start(10, "s2");
    d.recording(11);
    assert!(d.recorded(wav(), 4_000).is_empty(), "a recording while the new one runs is a late one");
    assert_eq!(d.view(true, 0).state, "recording");
    assert_eq!(d.view(true, 0).session, "s2");
    d.cancel();
    assert_eq!(d.view(true, 0).session, "", "idle belongs to no chat");
}

/// Settings: switched off nothing starts; the check shows the enclave's state; on again it is checked.
#[test]
fn the_switch_and_the_status() {
    let enclave = Enclave::new();
    let mut d = Dictation::default();
    assert_eq!(d.view(true, 0).status_kind, "unknown");
    assert_eq!(d.view(false, 0).status.unwrap().key, "dictate.status.unavailable");
    assert_eq!(d.check(0), vec![Do::FetchBundle]);
    assert_eq!(d.view(true, 0).status_kind, "checking");
    assert!(d.check(1).is_empty(), "one check at a time");
    d.bundle(b"{}".to_vec());
    d.attested(5, Some(enclave.attested()));
    let v = d.view(true, 10);
    assert_eq!((v.status_kind.as_str(), v.status.unwrap().en.as_str()), ("connected", "Connected · sealed"));
    assert_eq!(d.view(true, 5 + ATTESTED_FOR_MS).status_kind, "unknown", "checked too long ago");
    // off: no microphone, no check, a running dictation ends
    d.start(20, "s1");
    assert_eq!(d.set_on(false), vec![Do::Drop]);
    let v = d.view(true, 30);
    assert_eq!((v.state.as_str(), v.on, v.status_kind.as_str()), ("off", false, "off"));
    assert!(d.start(40, "s1").is_empty());
    assert!(d.check(ATTESTED_FOR_MS * 2).is_empty());
    d.set_on(true);
    assert_eq!(d.view(true, 50).state, "idle");
    // a failed check shows red
    let mut d = Dictation::default();
    d.check(0);
    d.bundle(b"{}".to_vec());
    d.attested(1, None);
    assert_eq!(d.view(true, 2).status_kind, "failed");
    let mut d = Dictation::default();
    d.check(0);
    d.bundle_failed(Problem::Off);
    assert_eq!(d.view(true, 2).status.unwrap().key, "dictate.status.hub_off");
}

fn bundle_json() -> Vec<u8> {
    br#"{"domain":"router.inf6.tinfoil.sh","digest":"3F9A0C5BE1D24477A1C08E2F6B9D3E4A5C6B7D8E9F0A1B2C3D4E5F6A7B8C7C21","enclaveAttestationReport":{"format":"https://tinfoil.sh/predicate/sev-snp-guest/v2","body":"x"}}"#.to_vec()
}

/// Security: the facts of the sealed connection, and the test: a fresh attestation, then a second of silence
/// sealed there and back; passed when the answer opens, with its time.
#[test]
fn the_facts_and_the_test() {
    let enclave = Enclave::new();
    let mut d = Dictation::default();
    let keys = |d: &Dictation, now| d.view(true, now).facts.iter().map(|f| f.key.clone()).collect::<Vec<_>>();
    assert_eq!(keys(&d, 0), ["enclave", "model", "attestation", "hub"], "no code before a verified bundle");
    assert!(d.view(false, 0).facts.is_empty(), "nothing where dictation is not available");
    // the test starts with a fresh check, whatever was checked before
    assert_eq!(d.test(1_000), vec![Do::FetchBundle]);
    assert_eq!(d.view(true, 1_000).test_state, "running");
    assert!(d.test(1_001).is_empty(), "one test at a time");
    d.bundle(bundle_json());
    let out = d.attested(1_200, Some(enclave.attested()));
    let sealed = match out.as_slice() { [Do::SendTest(s)] => s.clone(), other => panic!("{other:?}") };
    let (form, answer) = enclave.answer(&sealed, r#"{"text":""}"#);
    assert!(form.windows(4).any(|w| w == b"RIFF"), "the silent clip");
    d.test_sent(2_900, answer);
    let v = d.view(true, 61_200 + 1_200);
    assert_eq!((v.test_state.as_str(), v.test_result.unwrap().en.as_str()), ("passed", "✓ sealed round trip · 1.9 s"));
    let facts = v.facts;
    assert_eq!(facts.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["enclave", "model", "attestation", "code", "hub"]);
    assert_eq!(facts[0].value.en, "Tinfoil · AMD SEV-SNP");
    assert_eq!((facts[2].value.en.as_str(), facts[2].dot.as_str()), ("verified · 1 min ago", "connected"));
    assert_eq!(facts[3].value.en, "3f9a…7c21");
    assert_eq!(facts[3].href.as_deref(), Some("https://search.sigstore.dev/?hash=3f9a0c5be1d24477a1c08e2f6b9d3e4a5c6b7d8e9f0a1b2c3d4e5f6a7b8c7c21"));
    assert_eq!(facts[4].value.en, "relays ciphertext only");
    // a failed attestation fails the test, sends nothing, and shows red
    let mut d = Dictation::default();
    d.test(0);
    d.bundle(bundle_json());
    assert!(d.attested(1, None).is_empty());
    let v = d.view(true, 2);
    assert_eq!((v.test_state.as_str(), v.test_result.unwrap().key.as_str()), ("failed", "dictate.not_verified"));
    assert_eq!(v.facts.iter().find(|f| f.key == "attestation").unwrap().dot, "failed");
    assert!(!v.facts.iter().any(|f| f.key == "code"));
    // an answer that does not open fails it too; offline as well
    let mut d = Dictation::default();
    d.test(0);
    d.bundle(bundle_json());
    d.attested(1, Some(enclave.attested()));
    d.test_sent(2, Sent::Answered { nonce: "00".repeat(32), body: vec![0, 0, 0, 1, 7] });
    assert_eq!(d.view(true, 3).test_state, "failed");
    let mut d = Dictation::default();
    d.test(0);
    d.bundle_failed(Problem::Offline);
    assert_eq!(d.view(true, 1).test_result.unwrap().key, "dictate.offline");
    // a bundle that says nothing usable shows no code and no hardware
    let mut d = Dictation::default();
    d.test(0);
    d.bundle(br#"{"digest":"zz<script>"}"#.to_vec());
    d.attested(1, Some(enclave.attested()));
    let v = d.view(true, 2);
    assert!(!v.facts.iter().any(|f| f.key == "code"));
    assert_eq!(v.facts[0].value.en, "Tinfoil");
    // switched off: no test, no facts
    let mut d = Dictation::default();
    d.set_on(false);
    assert!(d.test(0).is_empty());
    assert!(d.view(true, 0).facts.is_empty());
}

#[test]
fn the_silent_clip_is_one_second_of_wav() {
    let w = silence();
    assert_eq!(w.len(), 44 + 32_000);
    assert_eq!(&w[..4], b"RIFF");
    assert!(w[44..].iter().all(|b| *b == 0));
    assert_eq!(seconds(1_949), "1.9 s");
    assert_eq!((ago(100_000, 100_000).as_str(), ago(400_000, 100_000).as_str(), ago(8_000_000, 0).as_str()), ("just now", "5 min ago", "2 h ago"));
}
