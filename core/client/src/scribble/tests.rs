use super::*;
use trommi_core::board_items::Point;

const ME: [u8; 32] = [1; 32];
const YOU: [u8; 32] = [2; 32];

fn points(xy: &[(i32, i32)]) -> String {
    let pts = xy.iter().zip(0u32..).map(|((x, y), i)| Point { x: x * 16, y: y * 16, t: i * 5, force: 255, azimuth: 0, altitude: 0 }).collect();
    Ink::new(pts, false, true).unwrap().to_base64url()
}

fn held(sender: [u8; 32], seq: u64, content: Value) -> Held {
    let payload = crate::codec::encode_body(crate::codec::BodyKind::BoardItem, content.as_object().unwrap()).unwrap();
    Held { sender: DeviceId::new(sender), seq, hash: Hash32::new([seq as u8; 32]), got: Got::Body(SecretBytes::new(payload.into_bytes())) }
}

fn id(sender: [u8; 32], seq: u64, index: u32) -> String {
    shape_key(&ShapeId { sender: DeviceId::new(sender), seq, index })
}

#[test]
fn every_client_finds_the_same_board() {
    assert_eq!(team_board("main"), "desk/6d61696e000000000000000000000004");
    assert_eq!(team_board(""), "desk/6d61696e000000000000000000000004");
    assert_eq!(team_board("all"), "desk/616c6c2d6465736b7300000000000009");
    assert_eq!(team_board("00112233445566778899aabbccddeeff"), "desk/00112233445566778899aabbccddeeff");
    assert_eq!(board_of("desk/00112233445566778899aabbccddeeff").map(|b| b.as_bytes()[15]), Some(0xff));
    assert!(board_of("desk/0011").is_none() && board_of("desk/00112233445566778899AABBCCDDEEFF").is_none() && board_of("main").is_none());
    assert_eq!(board_in_view("main").map(|b| hex(b.as_bytes())), Some("6d61696e000000000000000000000004".into()));
}

#[test]
fn items_add_up_whatever_order_their_writers_came_in() {
    let mut pad = Pad::new(BoardId::new([9; 16]));
    pad.step = Step::Ready;
    // an erase and a move of shapes that are not there yet count when they come
    pad.apply(held(YOU, 1, json!({ "content_type": "erase", "stroke_ids": [id(ME, 2, 1)] })), 10);
    pad.apply(held(YOU, 2, json!({ "content_type": "move", "stroke_ids": [id(ME, 2, 0)], "offset": [1, 1] })), 20);
    pad.apply(held(ME, 2, json!({ "content_type": "strokes", "strokes": [
        { "tool": "marker", "color": "yellow", "width": 18, "points": points(&[(0, 0), (4, 4)]) },
        { "tool": "text", "at": [1, 2], "text": "gone", "size": 20, "color": "ink" },
    ] })), 30);
    let shapes: Vec<&BoardShape> = pad.shapes.values().collect();
    assert_eq!(shapes.len(), 1);
    assert_eq!((shapes[0].id.clone(), shapes[0].tool.as_str(), shapes[0].pts.clone()), (id(ME, 2, 0), "marker", vec![1.0, 1.0, 5.0, 5.0]));
    assert_eq!((shapes[0].sim, shapes[0].f.clone(), shapes[0].width), (true, vec![1.0, 1.0], Some(18.0)));
    // an item its writer's number covers is skipped; a body the core refuses adds nothing
    pad.apply(held(ME, 2, json!({ "content_type": "erase", "stroke_ids": [id(ME, 2, 0)] })), 40);
    pad.apply(Held { got: Got::Body(SecretBytes::new(b"{\"content_type\":\"nonsense\"}".to_vec())), ..held(ME, 3, json!({ "content_type": "erase", "stroke_ids": [id(ME, 2, 0)] })) }, 40);
    assert_eq!(pad.shapes.len(), 1);
    assert_eq!((pad.applied, pad.last_at, pad.heads[&DeviceId::new(ME)].seq), (3, 30, 2));
    assert_eq!(pad.state(), "ready");
    // an item of a newer version: what it did is not shown, and no snapshot is written
    let newer = SecretBytes::new(br#"{"schema_version":3,"content_type":"strokes","strokes":[]}"#.to_vec());
    pad.apply(Held { got: Got::Body(newer), ..held(YOU, 3, json!({ "content_type": "erase", "stroke_ids": [id(ME, 2, 0)] })) }, 50);
    assert_eq!(pad.state(), "newer_version");
    assert!(pad.board.snapshot(&pad.heads.iter().map(|(w, h)| (*w, *h)).collect::<Vec<_>>()).is_err());
}

#[test]
fn a_snapshot_loads_into_the_same_board() {
    let mut pad = Pad::new(BoardId::new([9; 16]));
    pad.step = Step::Ready;
    pad.apply(held(ME, 1, json!({ "content_type": "strokes", "strokes": [{ "tool": "pen", "color": "ink", "width": 4, "points": points(&[(0, 0), (1, 1)]) }] })), 1);
    pad.apply(held(YOU, 4, json!({ "content_type": "move", "stroke_ids": [id(ME, 9, 0)], "offset": [2, 0] })), 2);
    let frontier: Vec<(DeviceId, Head)> = pad.heads.iter().map(|(w, h)| (*w, *h)).collect();
    let file = gzip::gzip(pad.board.snapshot(&frontier).unwrap().expose());
    let loaded = Board::from_snapshot(&gzip::gunzip(&file, MAX_SNAPSHOT_LEN).unwrap(), &frontier).unwrap();
    let mut again = Pad::new(pad.id);
    again.reset(loaded, &frontier);
    assert_eq!(again.shapes, pad.shapes);
    // what was kept for a shape not yet there holds across the snapshot
    for p in [&mut pad, &mut again] {
        p.apply(held(ME, 9, json!({ "content_type": "strokes", "strokes": [{ "tool": "pen", "color": "ink", "width": 4, "points": points(&[(0, 0)]) }] })), 3);
    }
    assert_eq!(again.shapes, pad.shapes);
    assert_eq!(again.shapes.values().last().map(|s| s.pts.clone()), Some(vec![2.0, 0.0]));
}

#[test]
fn a_live_stroke_grows_and_goes() {
    let mut scribble = Scribble::default();
    let board = BoardId::new([9; 16]);
    scribble.focus(Some(board));
    let you = DeviceId::new(YOU);
    let piece = |number: u32, xy: &[(i32, i32)]| json!({ "stroke": trommi_core::ids::base64url_encode(&[5; 16]), "number": number, "tool": "pen", "color": "blue", "width": 32, "points": points(xy) }).to_string();
    scribble.piece(&you, &board, piece(1, &[(0, 0)]).as_bytes(), 100);
    scribble.piece(&you, &board, piece(2, &[(1, 1), (2, 2)]).as_bytes(), 200);
    scribble.piece(&you, &board, b"not a piece", 200);
    let pad = &scribble.pads[&board];
    let (shape, at) = pad.live.values().next().unwrap();
    assert_eq!((shape.pts.clone(), shape.width, shape.live, *at), (vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0], Some(2.0), true, 200));
    assert_eq!(scribble.wake(300), Some(200 + LIVE_MS));
    assert!(scribble.take_dirty());
    // the finished stroke names the pieces it replaces
    let mut pad = scribble.pads.remove(&board).unwrap();
    pad.step = Step::Ready;
    let finished = json!({ "content_type": "strokes", "strokes": [{ "tool": "pen", "color": "blue", "width": 2, "live": hex(&[5; 16]), "points": points(&[(0, 0), (1, 1), (2, 2)]) }] });
    pad.apply(held(YOU, 1, finished), 300);
    assert!(pad.live.is_empty());
    assert_eq!(pad.shapes.len(), 1);
}
