use super::edge::{self, Packet};
use super::sim::Sim;
use super::*;
use crate::views::{Phase, ViewPart};

fn start() -> Event {
    Event::Start {
        config: Config {
            platform: "test".into(),
            version: "0".into(),
            seed: 1,
            ..Default::default()
        },
    }
}

#[test]
fn an_empty_device_starts_signed_out() {
    let mut sim = Sim::new(1_000);
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.view, *sim.app.view(), "the shell's fold is the core's");
    // the first render is whole, and comes before the I/O
    assert!(
        matches!(&sim.log[0], Effect::Render { delta } if matches!(delta.parts[0], ViewPart::Whole { .. }))
    );
    assert!(matches!(
        &sim.log[1],
        Effect::Store {
            op: StoreOp::DeviceLoad,
            ..
        }
    ));
}

#[test]
fn a_device_with_state_opens_and_survives_a_restart() {
    let mut sim = Sim::new(1_000);
    sim.device = (3, [(b"k".to_vec(), b"v".to_vec())].into_iter().collect());
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Opening);
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Opening);
}

#[test]
fn events_before_start_and_stray_answers_only_log() {
    let mut app = App::new();
    let out = app.handle(0, Event::Wake);
    assert!(out
        .iter()
        .all(|e| matches!(e, Effect::Log { .. } | Effect::Render { .. })));
    app.handle(0, start());
    let out = app.handle(
        0,
        Event::Http {
            id: IoId(99),
            result: IoResult::Err(IoError::new(IoErrorKind::Offline)),
        },
    );
    assert!(matches!(
        out.as_slice(),
        [Effect::Log {
            level: LogLevel::Warn,
            ..
        }]
    ));
}

#[test]
fn bytes_cross_the_edge_beside_the_json() {
    let event = Event::Push {
        payload: vec![0, 1, 2, 255].into(),
    };
    let packet = edge::encode_event(&event);
    assert_eq!(packet.json, r#"{"type":"push","payload":0}"#);
    assert_eq!(packet.blobs, vec![vec![0, 1, 2, 255]]);
    assert_eq!(edge::decode_event(packet).unwrap(), event);
    // outside the edge, bytes are base64url
    assert_eq!(
        serde_json::to_string(&Bytes(vec![0, 1, 2, 255])).unwrap(),
        r#""AAEC_w""#
    );
    let effects = vec![Effect::SaveFile {
        name: "a".into(),
        media_type: "text/plain".into(),
        bytes: b"hi".to_vec().into(),
    }];
    let packet = edge::encode_effects(&effects);
    assert_eq!(packet.blobs, vec![b"hi".to_vec()]);
    assert_eq!(edge::decode_effects(packet).unwrap(), effects);
    assert!(edge::decode_event(Packet {
        json: r#"{"type":"push","payload":3}"#.into(),
        blobs: vec![]
    })
    .is_err());
    // a refused event never quotes what it held
    let why = edge::decode_event(Packet {
        json: r#"{"type":"action","action":{"do":"account","action":{"step":"sign_in","email":"a@b","password":7}}}"#.replace('7', r#"["hunter2"]"#),
        blobs: vec![],
    })
    .unwrap_err();
    assert!(!why.contains("hunter2"), "{why}");
}

#[test]
fn a_failed_device_write_ends_the_answer() {
    let mut sim = Sim::new(0);
    sim.device.0 = 5;
    sim.hub = Some(Box::new(|_, _| panic!("ran past the barrier")));
    let request = HttpRequest {
        method: "GET".into(),
        url: "https://hub.example/x".into(),
        headers: vec![],
        body: None,
        timeout_ms: 1,
        max_answer: HttpRequest::MAX_ANSWER_DEFAULT,
        class: HttpClass::Background,
        key: None,
    };
    sim.execute(vec![
        Effect::Store {
            id: IoId(1),
            op: StoreOp::DeviceApply {
                expected_revision: 4,
                put: vec![],
                delete: vec![],
            },
        },
        Effect::Http {
            id: IoId(2),
            request,
        },
    ]);
    assert_eq!(sim.log.len(), 1, "nothing after the failed write ran");
    assert_eq!(sim.device.0, 5);
}

#[test]
fn debug_never_shows_secrets() {
    use crate::account::{AccountAction, Secret};
    let action = crate::actions::Action::Account {
        action: AccountAction::SignIn {
            email: "a@example.org".into(),
            password: Secret("hunter2".into()),
        },
    };
    let event = Event::Action { action };
    let shown = format!("{event:?}");
    assert!(
        !shown.contains("hunter2") && !shown.contains("example.org"),
        "{shown}"
    );
    let header = Header {
        name: "Authorization".into(),
        value: "Bearer t0ken".into(),
    };
    assert!(!format!("{header:?}").contains("t0ken"));
    let packet = edge::encode_event(&event);
    assert!(!format!("{packet:?}").contains("hunter2"));
    let answer = PlatformAnswer::PasskeyCreated {
        credential_id: vec![1].into(),
        attestation_object: vec![2].into(),
        client_data_json: vec![3].into(),
        prf: vec![42; 32].into(),
    };
    assert_eq!(format!("{answer:?}"), "PasskeyCreated(<redacted>)");
    let mut view = crate::views::ViewModel::default();
    view.account.kit = vec!["apple".into(), "brave".into()];
    view.account.link = Some("https://app.trommi.com/join#s3cret".into());
    let shown = format!("{view:?}");
    assert!(
        !shown.contains("apple") && !shown.contains("s3cret"),
        "{shown}"
    );
}

#[test]
fn a_request_without_a_cap_gets_the_small_one() {
    let request: HttpRequest =
        serde_json::from_str(r#"{"method":"GET","url":"https://hub.example/x","timeout_ms":1}"#).unwrap();
    assert_eq!(request.max_answer, 64 << 10);
    let text = serde_json::to_string(&request).unwrap();
    assert!(text.contains(r#""max_answer":65536"#), "{text}");
    assert_eq!(
        serde_json::to_string(&IoErrorKind::TooLarge).unwrap(),
        r#""too_large""#
    );
}
