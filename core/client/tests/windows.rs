//! Several windows on one App (`app::windows`): the browser's tabs share one core, one device and one stream, and
//! each tab looks at its own page and gets its own renders.

mod common;

use common::{start, World};
use trommi_client_core::app::edge::{handle, Packet};
use trommi_client_core::app::{App, Effect, Event};
use trommi_client_core::views::{Focus, Screen, ViewPart};

fn looking_at(screen: Screen, visible: bool) -> Event {
    Event::Focus { focus: Focus { screen, visible, ..Default::default() } }
}

fn opens(log: &[Effect]) -> usize {
    log.iter().filter(|e| matches!(e, Effect::OpenStream { .. })).count()
}

#[test]
fn each_window_has_its_own_page_and_the_room_moves_in_all() {
    let mut world = World::new();
    world.agent_says("hello");
    let mut sim = world.sim();
    sim.send(start());
    sim.send(looking_at(Screen::Desk, true));
    let session = sim.view.sidebar.sessions[0].id.clone();
    let streams = opens(&sim.log);
    let writes = sim.device.0;

    // a second window: its first render is whole, and it opens nothing of its own
    let from = sim.log.len();
    sim.send_in(1, Event::WindowOpened);
    let first: Vec<_> = sim.log[from..].iter().filter_map(|e| if let Effect::Render { delta, window } = e { Some((*window, delta.parts.len(), matches!(delta.parts[0], ViewPart::Whole { .. }))) } else { None }).collect();
    assert_eq!(first, [(1, 1, true)], "only the new window is rendered, whole");
    assert_eq!(sim.app.windows(), [0, 1]);
    assert_eq!(sim.window_view(1).map(|v| v.sidebar.sessions.len()), Some(1));

    // window 1 opens the session's page, window 0 stays on the Desk
    sim.send_in(1, looking_at(Screen::Session { session_id: session.clone() }, true));
    assert!(sim.window_view(1).unwrap().sessions.contains_key(&session));
    assert!(sim.view.sessions.is_empty(), "window 0 still shows its Desk");
    assert_eq!(sim.app.window_view(0), Some(&sim.view));
    assert_eq!(sim.app.window_view(1), sim.window_view(1));

    // a question that comes stands on both Desks and in window 1's chat
    world.agent_asks("Ship it?");
    assert!(world.stream(&mut sim));
    assert_eq!(sim.view.desk.rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Ship it?"]);
    let w1 = sim.window_view(1).unwrap();
    assert_eq!(w1.desk.rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Ship it?"]);
    assert!(w1.sessions[&session].talk.iter().any(|r| r.kind == "ask"));
    assert_eq!((w1.phase, w1.sidebar.badge), (sim.view.phase, sim.view.sidebar.badge));
    // (one device, one stream: the second window wrote and opened nothing)
    assert_eq!(opens(&sim.log), streams, "no second stream");
    assert!(sim.device.0 >= writes);
    // what each shell folded is what the core holds for it
    assert_eq!(sim.app.window_view(0), Some(&sim.view));
    assert_eq!(sim.app.window_view(1), sim.window_view(1));
}

#[test]
fn a_chat_in_view_in_any_window_is_read_in_every_window() {
    let mut world = World::new();
    world.agent_says("hello there");
    let mut sim = world.sim();
    sim.send(start());
    sim.send(looking_at(Screen::Desk, true));
    let session = sim.view.sidebar.sessions[0].id.clone();
    sim.send_in(1, looking_at(Screen::Desk, true));
    world.agent_says("news");
    assert!(world.stream(&mut sim));
    assert!(sim.view.sidebar.sessions[0].unread && sim.window_view(1).unwrap().sidebar.sessions[0].unread);
    // window 1 shows the chat: read, in window 0's list too
    sim.send_in(1, looking_at(Screen::Session { session_id: session.clone() }, true));
    assert!(!sim.view.sidebar.sessions[0].unread, "read in window 0 as well");
    assert!(!sim.window_view(1).unwrap().sidebar.sessions[0].unread);
    // window 1 hidden: what comes now stays unread, though window 0 is in front
    sim.send_in(1, Event::Background);
    world.agent_says("later");
    assert!(world.stream(&mut sim));
    assert!(sim.view.sidebar.sessions[0].unread);
}

#[test]
fn the_stream_stays_open_while_one_window_is_in_front() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    sim.send(looking_at(Screen::Desk, true));
    sim.send_in(1, looking_at(Screen::Desk, true));
    let cancels = |sim: &trommi_client_core::app::sim::Sim| sim.log.iter().filter(|e| matches!(e, Effect::Cancel { .. })).count();
    let before = cancels(&sim);
    sim.send_in(0, Event::Background);
    assert_eq!(cancels(&sim), before, "window 1 is still in front: the stream stays");
    sim.send_in(1, Event::Background);
    assert!(cancels(&sim) > before, "no window in front: the stream closes");
    let streams = opens(&sim.log);
    sim.send_in(1, Event::Foreground);
    assert!(opens(&sim.log) > streams, "a window in front again opens it");
}

#[test]
fn a_closed_window_is_forgotten_and_gets_no_more_renders() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    sim.send_in(1, looking_at(Screen::Desk, true));
    sim.send_in(2, looking_at(Screen::Notes, true));
    assert_eq!(sim.app.windows(), [0, 1, 2]);
    sim.send_in(1, Event::WindowClosed);
    assert_eq!(sim.app.windows(), [0, 2]);
    let from = sim.log.len();
    world.agent_asks("Still there?");
    assert!(world.stream(&mut sim));
    let to: Vec<u32> = sim.log[from..].iter().filter_map(|e| if let Effect::Render { window, .. } = e { Some(*window) } else { None }).collect();
    assert!(to.contains(&0) && to.contains(&2) && !to.contains(&1), "{to:?}");
    // the current window closed: another becomes current; the last is never forgotten
    sim.send_in(2, Event::WindowClosed);
    sim.send_in(0, Event::WindowClosed);
    assert_eq!(sim.app.windows(), [0]);
    assert_eq!(sim.window_view(2).map(|v| v.desk.rows.len()), Some(1), "window 2 had what came before it closed");
}

#[test]
fn the_edge_reads_the_window_beside_the_type_and_names_it_on_the_render() {
    let mut app = App::new();
    let start = r#"{"type":"start","config":{"platform":"test","version":"0","seed":1}}"#;
    let out = handle(&mut app, 1, Packet { json: start.into(), blobs: Vec::new() }).expect("the start");
    assert!(out.json.contains(r#""type":"render""#) && !out.json.contains(r#""window""#), "window 0 is left out: {}", &out.json[..out.json.len().min(200)]);
    let focus = r#"{"type":"focus","window":3,"focus":{"screen":{"screen":"desk"}}}"#;
    let out = handle(&mut app, 2, Packet { json: focus.into(), blobs: Vec::new() }).expect("the focus");
    assert!(out.json.contains(r#""window":3"#), "{}", &out.json[..out.json.len().min(200)]);
    assert_eq!(app.windows(), [0, 3]);
    let wake = r#"{"type":"wake","window":3}"#;
    assert!(handle(&mut app, 3, Packet { json: wake.into(), blobs: Vec::new() }).is_ok(), "a window beside any event is read");
}
