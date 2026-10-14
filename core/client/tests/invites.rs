//! Letting a new device in by link through the App (`app::invites`): the link shows, the newcomer's Request is
//! found at the hub and accepted, both sides show the same six emoji, and the confirmation adds the newcomer; "they
//! don't match" burns the invite.

mod common;

use common::{new_peer, now, start, World};
use trommi_client_core::account::AccountAction;
use trommi_client_core::actions::Action;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::Event;
use trommi_client_core::views::Phase;
use trommi_core::invite::SignedOffer;

fn account(sim: &mut Sim, action: AccountAction) {
    sim.send(Event::Action { action: Action::Account { action } });
}

/// The newcomer reads the link and sends its Request to the stand-in hub; returns the invite's id (base64url).
fn newcomer_asks(world: &World, joiner: &mut common::Peer, link: &str) -> String {
    let mut hub = world.hub.borrow_mut();
    let (id, invite) = hub.invites.iter_mut().next().expect("the Offer was published");
    let offer = SignedOffer { offer: invite.offer.clone(), signature: invite.signature.clone(), mac: invite.mac.clone() };
    let request = joiner.join_request(link, &offer, now()).expect("a Request").signed_request;
    invite.requests.push(request);
    id.clone()
}

#[test]
fn a_device_is_let_in_when_the_emoji_match() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    account(&mut sim, AccountAction::InviteDevice);
    assert_eq!(sim.view.account.step, "invite", "{:?}", sim.view.account.problem);
    let link = sim.view.account.link.clone().expect("the link");
    assert!(!format!("{:?}", sim.view.account).contains(&link), "the link stays out of Debug");

    let (mut joiner, _store) = new_peer();
    let id = newcomer_asks(&world, &mut joiner, &link);
    sim.advance(3_100);
    assert_eq!(sim.view.account.step, "compare_emoji", "{:?}", sim.view.account.problem);
    let reveal = world.hub.borrow().invites[&id].reveal.clone().expect("the Reveal was published");
    let code = joiner.join_reveal(&reveal).expect("the code");
    let words: Vec<String> = sim.view.account.emoji.iter().map(|e| e.word.clone()).collect();
    assert_eq!(words, code.words().map(String::from).to_vec(), "both screens show the same emoji");

    let invite_id = sim.view.account.invite_id.clone().unwrap();
    account(&mut sim, AccountAction::ConfirmEmoji { invite_id, matches: true });
    for _ in 0..20 {
        world.stream(&mut sim);
        sim.advance(1_000);
        if sim.view.account.step == "none" {
            break;
        }
    }
    assert_eq!(sim.view.account.step, "none", "{:?}", sim.view.account);
    let (humans, _) = sim.app.engine().unwrap().roles();
    assert!(humans.contains(&joiner.id()), "the newcomer is in the room");
}

#[test]
fn emoji_that_do_not_match_burn_the_invite() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    account(&mut sim, AccountAction::InviteDevice);
    let link = sim.view.account.link.clone().expect("the link");
    let (mut joiner, _store) = new_peer();
    let id = newcomer_asks(&world, &mut joiner, &link);
    sim.advance(3_100);
    assert_eq!(sim.view.account.step, "compare_emoji");
    let invite_id = sim.view.account.invite_id.clone().unwrap();
    account(&mut sim, AccountAction::ConfirmEmoji { invite_id, matches: false });
    assert_eq!(sim.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("invite.mismatch"));
    assert!(!world.hub.borrow().invites.contains_key(&id), "burned at the hub");
    let (humans, _) = sim.app.engine().unwrap().roles();
    assert!(!humans.contains(&joiner.id()));
}
