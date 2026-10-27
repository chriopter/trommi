//! An outbox that halts on the hub's refusal is no silent one: the answer behind it says "Not sent yet: <code>" on
//! its card's page (AnswerView.not_sent), until the hub takes it.

mod common;

use common::hub::answer;
use common::{start, World};
use serde_json::json;
use trommi_client_core::actions::Action;
use trommi_client_core::app::io::HttpRequest;
use trommi_client_core::app::Event;
use trommi_client_core::engine::sync::BLOCKED_RETRY_MS;
use trommi_client_core::views::{Focus, Phase, Screen};

fn yes(card: &str) -> Action {
    Action::Answer {
        card_id: card.into(),
        choices: vec!["yes".into()],
        note: None,
        attachments: vec![],
        option_notes: Default::default(),
        trusted: false,
        marks: vec![],
        onward: false,
    }
}

fn card_in_view(card: &str) -> Event {
    Event::Focus {
        focus: Focus {
            screen: Screen::Card {
                card_id: card.into(),
            },
            visible: true,
            team: None,
            board: None,
            open_teams: Vec::new(),
            chats_query: None,
            calm: false,
            walk_chat: None,
            group_chat: None,
            rail_folded: false,
            walk_all: false,
        },
    }
}

#[test]
fn an_answer_behind_a_refused_envelope_says_it_is_not_sent_yet() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    world.agent_asks("Ship it?");
    assert!(world.stream(&mut sim));
    let card = sim.view.group.rows()[0].id.clone();
    sim.send(card_in_view(&card));
    // the hub refuses the envelope without a void record: the pump halts on it and keeps it
    world.hub.borrow_mut().fault = Some(Box::new(|_, request: &HttpRequest| {
        request.url.ends_with("/v1/envelopes").then(|| {
            Ok(answer(
                403,
                json!({ "error": "forbidden", "message": "refused", "voided": false }),
            ))
        })
    }));
    sim.send(Event::Action { action: yes(&card) });
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 1);
    let said = &sim.view.cards[&card].answer;
    assert_eq!(said.mode, "said");
    let waits = said
        .not_sent
        .as_ref()
        .expect("the answer says it is not sent");
    assert_eq!(
        (waits.key.as_str(), waits.en.as_str()),
        ("card.not_sent", "Not sent yet: forbidden")
    );
    assert!(
        sim.view.toasts.is_empty(),
        "no refusal for good: nothing rolled back"
    );
    // the hub takes it at the next try: the line goes
    world.hub.borrow_mut().fault = None;
    sim.advance(BLOCKED_RETRY_MS + 1_000);
    world.stream(&mut sim);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    let said = &sim.view.cards[&card].answer;
    assert_eq!((said.mode.as_str(), said.not_sent.as_ref()), ("said", None));
}
