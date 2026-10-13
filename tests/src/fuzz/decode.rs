//! Every decoder of the core on bytes from nowhere: none may panic, and whatever one accepts must come out of
//! its encoder again. Where the module promises a canonical encoding (the codec structs, the envelope), the
//! bytes that came in are the bytes that go out; elsewhere, encoding what was decoded and decoding that again
//! gives the same encoding (decode(encode(x)) == x, seen through the encoder).
//!
//! The first byte picks the decoder, the rest is its input.

use trommi_core::board::{ShapeId, Snapshot};
use trommi_core::board_items::{Board, Ink, ItemBody};
use trommi_core::chain::{self, Chains, Head, OwnChain};
use trommi_core::codec::{self, Decode, Encode};
use trommi_core::envelope::Envelope;
use trommi_core::files::{FileHead, ShareLink};
use trommi_core::hub_auth::{HubAddress, HubAuth};
use trommi_core::ids::{DeviceId, GroupId};
use trommi_core::invite::{InviteLink, Offer, Request, Reveal};
use trommi_core::mls::key_package::verify_key_package;
use trommi_core::mls::message::{EpochKey, TrommiMessage};
use trommi_core::mls::profile::{CommitNote, Cut, TrommiRoom, TrommiSession};
use trommi_core::mls::rules::RoomState;
use trommi_core::objects::{GateLog, Objects};
use trommi_core::push::WebPush;
use trommi_core::recovery::{RecoveryAuth, RecoveryLink, SealedKey};
use trommi_core::registers::{self, OwnIds, Registers};
use trommi_core::trail::{StrokePiece, WorkStep};

const MAX: usize = 1 << 20;

/// A struct of the codec: what it accepts it writes back byte for byte.
fn canonical<T: Decode + Encode>(what: &str, bytes: &[u8]) {
    if let Ok(value) = codec::decode::<T>(bytes, MAX) {
        let again = codec::encode(&value)
            .unwrap_or_else(|e| panic!("{what}: decoded but not encoded: {e:?}"));
        assert_eq!(again, bytes, "{what}: not canonical");
    }
}

/// A stored form: decoding what was encoded gives the same encoding.
fn stable<T>(
    what: &str,
    bytes: &[u8],
    from: impl Fn(&[u8]) -> Result<T, trommi_core::Error>,
    to: impl Fn(&T) -> Result<Vec<u8>, trommi_core::Error>,
) {
    if let Ok(value) = from(bytes) {
        let once = to(&value).unwrap_or_else(|e| panic!("{what}: decoded but not encoded: {e:?}"));
        let back = from(&once)
            .unwrap_or_else(|e| panic!("{what}: its own encoding does not decode: {e:?}"));
        let twice = to(&back).unwrap_or_else(|e| panic!("{what}: encoded once, not twice: {e:?}"));
        assert_eq!(once, twice, "{what}: decode(encode(x)) != x");
    }
}

fn text(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// A frontier taken from the front of the input: up to three writers with their heads.
fn frontier(bytes: &[u8]) -> Vec<(DeviceId, Head)> {
    let mut out = Vec::new();
    for (i, b) in bytes.iter().take(3).enumerate() {
        let device = DeviceId::new([*b; 32]);
        if out.iter().any(|(d, _): &(DeviceId, Head)| *d == device) {
            continue;
        }
        let head = Head {
            seq: u64::from(*b) + i as u64,
            hash: trommi_core::ids::Hash32::new([*b ^ 0x5a; 32]),
        };
        out.push((device, head));
    }
    out.sort_by_key(|(d, _)| *d);
    out
}

pub fn run(data: &[u8]) {
    let Some((&pick, bytes)) = data.split_first() else {
        return;
    };
    match pick % 40 {
        0 => {
            if let Ok(envelope) = Envelope::decode(bytes) {
                let again = envelope.encode().expect("a decoded envelope encodes");
                assert_eq!(again, bytes, "envelope: not canonical");
                let _ = envelope.hash();
                let _ = envelope.verify();
                let _ = envelope.is_oversize();
                if let Ok(pruned) = envelope.prune() {
                    let p = pruned.encode().expect("a pruned envelope encodes");
                    let back = Envelope::decode(&p).expect("a pruned envelope decodes");
                    assert_eq!(back.encode().expect("encodes"), p);
                    assert!(back.is_pruned());
                    assert_eq!(
                        pruned.hash().ok(),
                        envelope.hash().ok(),
                        "pruning changed the hash"
                    );
                }
            }
        }
        1 => {
            if let Ok(body) = ItemBody::decode(bytes) {
                let once = body.encode().expect("a decoded body encodes");
                let back = ItemBody::decode(once.expose()).expect("its own encoding decodes");
                assert_eq!(back, body, "item body: decode(encode(x)) != x");
                let _ = body.file_ids();
            }
        }
        2 => {
            if let Ok(ink) = Ink::unpack(bytes) {
                let packed = ink.pack();
                assert_eq!(Ink::unpack(&packed).expect("packed ink unpacks"), ink);
                let text = ink.to_base64url();
                assert_eq!(Ink::from_base64url(&text).expect("ink text reads"), ink);
            }
        }
        3 => {
            if let Some(t) = text(bytes) {
                if let Ok(ink) = Ink::from_base64url(t) {
                    assert_eq!(Ink::unpack(&ink.pack()).expect("packed ink unpacks"), ink);
                }
            }
        }
        4 => {
            if let Some(t) = text(bytes) {
                if let Ok(id) = ShapeId::parse(t) {
                    assert_eq!(
                        ShapeId::parse(&id.to_string()).expect("its text parses"),
                        id
                    );
                }
            }
        }
        5 => {
            if let Some(t) = text(bytes) {
                if let Ok(snapshot) = Snapshot::parse(t) {
                    if let Ok(value) = snapshot.value() {
                        let back = Snapshot::parse(&value).expect("its value parses");
                        assert_eq!(
                            back.value().expect("encodes again"),
                            value,
                            "snapshot register"
                        );
                    }
                }
            }
        }
        6 => {
            let (head, file) = bytes.split_at(bytes.len().min(3));
            let heads = frontier(head);
            if let Ok(board) = Board::from_snapshot(file, &heads) {
                let once = board
                    .snapshot(&heads)
                    .expect("a board read from a snapshot writes one");
                let back =
                    Board::from_snapshot(once.expose(), &heads).expect("its own snapshot reads");
                assert_eq!(back, board, "board snapshot: decode(encode(x)) != x");
            }
        }
        7 => stable("chains", bytes, Chains::from_bytes, Chains::to_bytes),
        8 => stable("own chain", bytes, OwnChain::from_bytes, OwnChain::to_bytes),
        9 => {
            if let Some(t) = text(bytes) {
                let _ = chain::parse_heads(t);
            }
        }
        10 => stable("objects", bytes, Objects::from_bytes, Objects::to_bytes),
        11 => stable("gate log", bytes, GateLog::from_bytes, GateLog::to_bytes),
        12 => {
            if let Ok(registers) = Registers::from_bytes(bytes) {
                let once = registers.to_bytes().expect("encodes");
                let back = Registers::from_bytes(&once).expect("its own encoding decodes");
                assert_eq!(back, registers, "registers: decode(encode(x)) != x");
            }
        }
        13 => stable(
            "own register ids",
            bytes,
            OwnIds::from_bytes,
            OwnIds::to_bytes,
        ),
        14 => {
            if let Ok(value) = registers::Value::parse(bytes) {
                if let Ok(payload) = value.payload() {
                    let back = registers::Value::parse(&payload).expect("its own payload parses");
                    assert_eq!(back, value, "register value: decode(encode(x)) != x");
                }
            }
        }
        15 => {
            if let Ok(head) = FileHead::decode(bytes) {
                assert_eq!(&head.encode()[..], bytes, "file head: not canonical");
            }
        }
        16 => {
            if let Some(t) = text(bytes) {
                if let Ok(link) = ShareLink::parse(t) {
                    let once = link.to_text();
                    let back = ShareLink::parse(text(once.expose()).expect("utf-8"))
                        .expect("its text parses");
                    assert_eq!(back.to_text(), once, "share link");
                }
            }
        }
        17 => {
            if let Some(t) = text(bytes) {
                if let Ok(link) = InviteLink::parse(t) {
                    let once = link.to_text();
                    let back = InviteLink::parse(text(once.expose()).expect("utf-8"))
                        .expect("its text parses");
                    assert_eq!(back.to_text(), once, "invite link");
                }
            }
        }
        18 => {
            if let Ok(address) = HubAddress::from_bytes(bytes) {
                assert_eq!(address.as_str().as_bytes(), bytes);
            }
        }
        19 => {
            if HubAuth::decode(bytes).is_ok() {
                canonical::<HubAuth>("hub auth", bytes);
            }
        }
        20 => {
            if Offer::decode(bytes).is_ok() {
                canonical::<Offer>("offer", bytes);
            }
        }
        21 => {
            if Request::decode(bytes).is_ok() {
                canonical::<Request>("request", bytes);
            }
        }
        22 => {
            if Reveal::decode(bytes).is_ok() {
                canonical::<Reveal>("reveal", bytes);
            }
        }
        23 => stable("web push", bytes, WebPush::decode, WebPush::encode),
        24 => stable("work step", bytes, WorkStep::decode, WorkStep::encode),
        25 => stable(
            "stroke piece",
            bytes,
            StrokePiece::decode,
            StrokePiece::encode,
        ),
        26 => stable(
            "sealed key",
            bytes,
            SealedKey::from_bytes,
            SealedKey::to_bytes,
        ),
        27 => stable(
            "recovery auth",
            bytes,
            RecoveryAuth::from_bytes,
            RecoveryAuth::to_bytes,
        ),
        28 => stable(
            "recovery link",
            bytes,
            RecoveryLink::from_bytes,
            RecoveryLink::to_bytes,
        ),
        29 => canonical::<TrommiRoom>("trommi room", bytes),
        30 => canonical::<TrommiSession>("trommi session", bytes),
        31 => canonical::<Cut>("cut", bytes),
        32 => canonical::<CommitNote>("commit note", bytes),
        33 => canonical::<RoomState>("room state", bytes),
        34 => canonical::<EpochKey>("epoch key", bytes),
        35 => canonical::<TrommiMessage>("trommi message", bytes),
        36 => canonical::<Head>("head", bytes),
        37 => {
            let _ = verify_key_package(bytes);
        }
        38 => {
            if let Ok(group) = GroupId::from_bytes(bytes) {
                assert_eq!(group.as_bytes(), bytes);
            }
        }
        _ => {
            if let Some(t) = text(bytes) {
                if let Ok(id) = trommi_core::account::AccountId::parse(t) {
                    let shown = id.to_string();
                    let back =
                        trommi_core::account::AccountId::parse(&shown).expect("its text parses");
                    assert_eq!(back.to_string(), shown);
                }
                let _ = trommi_core::account::parse_kit_words(t);
                let _ = trommi_core::account::parse_recovery_code(t);
                let _ = trommi_core::account::check_password(t);
            }
        }
    }
}

/// Small valid inputs for every decoder, each behind the byte that picks it: the seeds of the corpus
/// (`tests/fuzz/seeds/core_decode`, written by `fuzz_props::write_seeds`).
pub fn seeds() -> Vec<Vec<u8>> {
    use crate::content::{history, Dice};
    use crate::room::{device, room, seeded, session, World};
    use trommi_core::crypto::Secret;
    use trommi_core::envelope::{Draft, ObjectType, Urgency};
    use trommi_core::ids::{BoardId, Hash32, RegisterId, RoomId, SessionId};

    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut put = |pick: u8, bytes: &[u8]| {
        let mut v = vec![pick];
        v.extend_from_slice(bytes);
        out.push(v);
    };
    let mut world = World::new();
    let zero = Hash32::ZERO.to_base64url();
    let card = format!(
        r#"{{"schema_version":2,"title":"t","object_version":1,"previous_version_hash":"{zero}"}}"#
    );
    let mut sealed = Vec::new();
    sealed.push(world.post(
        3,
        session(),
        &Draft::first_version(ObjectType::Card, Urgency::High, card.as_bytes()).expect("draft"),
    ));
    sealed.push(world.post(
        1,
        room(),
        &Draft::first_version(ObjectType::Note, Urgency::Low, card.as_bytes()).expect("draft"),
    ));
    let register = br#"{"name":"desk/a","value":{"goals":"a\nb"},"lamport":1}"#;
    sealed.push(world.post(
        1,
        room(),
        &Draft::register(RegisterId::new([5; 16]), register),
    ));
    sealed.push(world.post(
        3,
        session(),
        &Draft::session_chat(
            SessionId::new([2; 16]),
            crate::room::device(1),
            br#"{"text":"hi"}"#,
        ),
    ));
    let mut dice = Dice(seeded(7));
    let items = history(&mut dice, &[device(1), device(2)], 6).expect("a history");
    for item in &items {
        put(1, &item.payload);
        sealed.push(world.post(
            1,
            room(),
            &Draft::board_item(BoardId::new([9; 16]), &item.payload),
        ));
    }
    for receipt in &sealed {
        let bytes = receipt.envelope().encode().expect("encodes");
        put(0, &bytes);
        if let Ok(pruned) = receipt.envelope().prune() {
            put(0, &pruned.encode().expect("encodes"));
        }
    }
    for chains in world.chains.values() {
        put(7, &chains.to_bytes().expect("encodes"));
    }
    for objects in world.objects.values() {
        put(10, &objects.to_bytes().expect("encodes"));
    }
    put(14, register);
    let mut board = Board::new();
    for item in &items {
        let _ = board.apply(
            item.sender,
            item.seq,
            ItemBody::decode(&item.payload).expect("reads"),
        );
        if let Ok(ItemBody::Strokes(shapes)) = ItemBody::decode(&item.payload) {
            for shape in shapes {
                if let trommi_core::board_items::ShapeKind::Stroke(stroke) = shape.kind {
                    put(2, &stroke.ink.pack());
                    put(3, stroke.ink.to_base64url().as_bytes());
                }
            }
        }
    }
    put(4, b"AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA/12/0");
    let link = InviteLink::new(
        "https://app.trommi.com",
        HubAddress::parse("https://hub.example").expect("address"),
        RoomId::new([3; 32]),
        Secret::new([4; 32]),
        1_800_000_000_000,
    )
    .expect("a link");
    put(17, link.to_text().expose());
    put(18, b"https://hub.example");
    put(39, b"correct horse battery staple");
    out
}
