//! The Scribble Board through the App (`crate::scribble`): a board read when it comes into view, what the other
//! device drew on it, this device's strokes shown at once and then as the hub took them, an erase and a move, a
//! stroke the other device is still drawing, a snapshot written and the board read again from it alone, and an item
//! the hub refuses taken back with a toast.

mod common;

use common::hub::answer;
use common::{start, World};
use serde_json::{json, Value};
use trommi_client_core::actions::Action;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::Event;
use trommi_client_core::codec::{self, BodyKind};
use trommi_client_core::scribble;
use trommi_client_core::views::{BoardShape, Focus, Phase, Screen};
use trommi_core::board_items::{Ink, ItemBody, Point};
use trommi_core::crypto::SecretBytes;
use trommi_core::device::Draft;
use trommi_core::ids::BoardId;

const BOARD: &str = "desk/00112233445566778899aabbccddeeff";

fn board() -> BoardId {
    scribble::board_of(BOARD).expect("a board id")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Packed points at these places (board units), 10 ms apart.
fn points(xy: &[(i32, i32)]) -> String {
    let pts = xy.iter().zip(0u32..).map(|((x, y), i)| Point { x: x * 16, y: y * 16, t: i * 10, force: 64, azimuth: 0, altitude: 0 }).collect();
    Ink::new(pts, false, false).expect("points").to_base64url()
}

fn stroke(xy: &[(i32, i32)]) -> Value {
    json!({ "tool": "pen", "color": "ink", "width": 4, "points": points(xy) })
}

/// A board item of the other device, as its own core seals it.
fn item(content: Value) -> Draft {
    let payload = codec::encode_body(BodyKind::BoardItem, content.as_object().expect("an object")).expect("a board item");
    Draft::BoardItem { board: board(), payload: SecretBytes::new(payload.into_bytes()) }
}

fn look(sim: &mut Sim, board: Option<&str>) {
    sim.send(Event::Focus { focus: Focus { board: board.map(String::from), ..Default::default() } });
}

fn shapes(sim: &Sim) -> Vec<BoardShape> {
    sim.view.board.as_ref().expect("a board in view").shapes.clone()
}

fn draw(sim: &mut Sim, local_id: &str, content: Value) {
    sim.send(Event::Action { action: Action::BoardItem { board_id: BOARD.into(), local_id: local_id.into(), content } });
}

#[test]
fn a_board_is_read_drawn_on_snapshotted_and_read_again() {
    let mut world = World::new();
    let founder = hex(world.founder.id().as_bytes());
    let me = hex(world.me.as_bytes());
    // the other device drew before this one looks: a stroke and a sticky note
    world.founder_writes(&item(json!({ "content_type": "strokes", "strokes": [
        stroke(&[(0, 0), (10, 10)]),
        { "tool": "sticky", "at": [5, 5], "text": "hi", "size": 20, "color": "yellow" },
    ] })));
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(sim.view.board.is_none(), "no board in view");

    // in view: read from the hub (no snapshot yet), the two shapes as drawn
    look(&mut sim, Some(BOARD));
    let view = sim.view.board.clone().expect("the board");
    assert_eq!((view.board_id.as_str(), view.state.as_str()), (BOARD, "ready"));
    assert!(world.hub.borrow().seen.iter().any(|s| s.starts_with("GET /v1/boards/") && s.contains("after_change=0")));
    let got = shapes(&sim);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!((got[0].tool.as_str(), got[0].by.as_str(), got[0].pts.clone()), ("pen", founder.as_str(), vec![0.0, 0.0, 10.0, 10.0]));
    assert_eq!((got[0].t.clone(), got[0].f.clone(), got[0].width, got[0].color.as_deref()), (vec![0, 10], vec![0.251, 0.251], Some(4.0), Some("ink")));
    assert_eq!((got[1].tool.as_str(), got[1].text.as_deref(), got[1].pts.clone(), got[1].size), ("sticky", Some("hi"), vec![5.0, 5.0], Some(20.0)));
    let theirs = got[0].id.clone();
    assert!(theirs.starts_with(&format!("{founder}/")) && theirs.ends_with("/0"));
    // the Desk without a board shows none
    look(&mut sim, None);
    assert!(sim.view.board.is_none());
    look(&mut sim, Some(BOARD));

    // this device draws: shown at once with its local id, then as the hub took it
    draw(&mut sim, "s1", json!({ "content_type": "strokes", "strokes": [stroke(&[(20, 20), (30, 30)])] }));
    let mine = shapes(&sim).into_iter().find(|s| s.local.is_some()).expect("shown at once");
    assert_eq!((mine.local.as_deref(), mine.by.as_str()), (Some("s1/0"), me.as_str()));
    world.stream(&mut sim);
    let mine = shapes(&sim).into_iter().find(|s| s.id == mine.id).expect("still there");
    assert_eq!((mine.local, mine.pts.clone()), (None, vec![20.0, 20.0, 30.0, 30.0]));

    // the other device reads it as its core reads a board item
    {
        let hub = world.hub.borrow();
        let common::hub::Item::Envelope { change, bytes, .. } = hub.order.iter().rev().find(|i| matches!(i, common::hub::Item::Envelope { sender, .. } if *sender == world.me)).cloned().expect("the item at the hub") else { unreachable!() };
        drop(hub);
        common::sync(&world.hub.borrow(), &mut world.founder);
        let read = world.founder.receive_envelope(&bytes, change, false, None, common::now()).expect("read");
        match ItemBody::decode(read.body.expect("opened").payload()).expect("a board item") {
            ItemBody::Strokes(list) => assert_eq!(list.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    // erased: gone at once, and for good
    draw(&mut sim, "e1", json!({ "content_type": "erase", "stroke_ids": [theirs] }));
    assert!(!shapes(&sim).iter().any(|s| s.id == theirs));
    world.stream(&mut sim);
    assert!(!shapes(&sim).iter().any(|s| s.id == theirs));
    // moved: at once, and as the board takes it
    draw(&mut sim, "m1", json!({ "content_type": "move", "stroke_ids": [mine.id], "offset": [1.5, -2] }));
    let moved = |sim: &Sim| shapes(sim).into_iter().find(|s| s.id == mine.id).expect("there").pts;
    assert_eq!(moved(&sim), vec![21.5, 18.0, 31.5, 28.0]);
    world.stream(&mut sim);
    assert_eq!(moved(&sim), vec![21.5, 18.0, 31.5, 28.0]);
    assert_eq!(shapes(&sim).len(), 2);

    // the other device is drawing: its pieces grow a live stroke, its finished stroke replaces them
    let live = [7u8; 16];
    let piece = |number: u32, xy: &[(i32, i32)]| {
        json!({ "stroke": trommi_core::ids::base64url_encode(&live), "number": number, "tool": "pen", "color": "blue", "width": 64, "points": points(xy) }).to_string().into_bytes()
    };
    world.founder_piece(&board(), &piece(1, &[(40, 40), (41, 41)]));
    world.stream(&mut sim);
    let drawing = shapes(&sim).into_iter().find(|s| s.live).expect("a live stroke");
    assert_eq!((drawing.id.clone(), drawing.pts.len()), (format!("live:{founder}/{}", hex(&live)), 4));
    world.founder_piece(&board(), &piece(2, &[(42, 42)]));
    world.stream(&mut sim);
    assert_eq!(shapes(&sim).into_iter().find(|s| s.live).expect("still live").pts, vec![40.0, 40.0, 41.0, 41.0, 42.0, 42.0]);
    let mut finished = stroke(&[(40, 40), (41, 41), (42, 42)]);
    finished["live"] = json!(hex(&live));
    world.founder_writes(&item(json!({ "content_type": "strokes", "strokes": [finished] })));
    world.stream(&mut sim);
    let now = shapes(&sim);
    assert!(!now.iter().any(|s| s.live), "the finished stroke replaced its pieces");
    assert_eq!(now.len(), 3);

    // a snapshot now (a desk's delete asks for one at once): declared, uploaded, the register written, bound
    let fx = sim.app.board_work(sim.now_ms, BOARD, true);
    sim.run(fx);
    {
        let hub = world.hub.borrow();
        assert_eq!(hub.frontiers.len(), 2, "{:?}", hub.frontiers);
        assert!(hub.frontiers[0].1.get("snapshot").is_none() && hub.frontiers[1].1.get("snapshot").is_some());
        assert_eq!(hub.frontiers[1].1["frontier"].as_object().map(|f| f.len()), Some(2), "both writers");
        assert_eq!(hub.files.len(), 1, "the snapshot file");
    }
    world.stream(&mut sim);
    let known = sim.app.board_known(BOARD).expect("read");
    assert_eq!((known.shapes.len(), known.notes, known.snapshot_empty), (3, 1, false));

    // read again on a fresh start, from the snapshot alone (the hub pruned every item behind it)
    let before = shapes(&sim);
    let pruned = world.hub.borrow().change;
    world.hub.borrow_mut().board_pruned = pruned;
    sim.restart();
    sim.send(start());
    look(&mut sim, Some(BOARD));
    assert!(world.hub.borrow().seen.iter().any(|s| s.starts_with("GET /v1/files/")), "the snapshot's file was read");
    assert_eq!(sim.view.board.as_ref().map(|b| b.state.as_str()), Some("ready"));
    assert_eq!(shapes(&sim), before);

    // a later stroke of the other device comes on top of it, live
    world.founder_writes(&item(json!({ "content_type": "strokes", "strokes": [stroke(&[(50, 50), (60, 60)])] })));
    world.stream(&mut sim);
    assert_eq!(shapes(&sim).len(), 4);
}

#[test]
fn an_item_the_hub_refuses_is_taken_back() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    sim.send(Event::Focus { focus: Focus { screen: Screen::Scribble { board_id: BOARD.into() }, ..Default::default() } });
    assert_eq!(sim.view.board.as_ref().map(|b| b.state.as_str()), Some("ready"));
    let status = trommi_client_core::hub::status_of("forbidden").expect("a status");
    let mut once = true;
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request| {
        (request.method == "POST" && request.url.ends_with("/v1/envelopes") && std::mem::take(&mut once))
            .then(|| Ok(answer(status, json!({ "error": "forbidden", "message": "refused", "voided": true }))))
    }));
    draw(&mut sim, "s1", json!({ "content_type": "strokes", "strokes": [stroke(&[(1, 1), (2, 2)])] }));
    assert!(shapes(&sim).is_empty(), "taken back");
    assert!(sim.view.toasts.iter().any(|t| t.text.key == "action.refused"), "{:?}", sim.view.toasts);
    // a piece that is no piece goes nowhere, quietly
    sim.send(Event::Action { action: Action::StrokePiece { board_id: BOARD.into(), piece: json!({ "stroke": "zz" }) } });
    assert!(shapes(&sim).is_empty());
}
