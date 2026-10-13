//! The connector on a network that cuts its live stream again and again (a train, a flaky Wi-Fi, a proxy that ends
//! long requests): each stream opens and brings what happened, then the connection is gone. The connector opens
//! the next one about a second later every time, as on the first cut. Before, its pause doubled with every stream
//! that ended within 30 seconds, and after a failed one it never went back: a few cuts, and the agent was away (and
//! deaf to the person's answers) for up to a minute and a half at a time. Found by the torture run
//! (tests/client/mega/torture.rs, seed 102). A hub that ends every stream at once, bringing nothing, is still asked
//! more and more slowly (dead_hub.rs).
#[allow(dead_code)]
#[path = "../client/mega/chaos.rs"]
mod chaos;
mod common;

use chaos::{Knobs, Proxy};
use common::process::Seat;
use common::{HubProc, Human, TempDir};
use serde_json::json;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_cut_after_each_event_is_opened_again_at_once() {
    let proxy = Proxy::start(7).await;
    let hub = HubProc::start_in_with(TempDir::new("hub"), 0, &[("HUB_URL", proxy.url.as_str())]).await;
    proxy.set_upstream(hub.port);
    let mut human = Human::found(&proxy.url).await;
    let seat = Seat::new(&proxy.url);
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let join = seat.join(&link);
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((joined, log), group) = tokio::join!(join, admit);
    assert!(joined, "the join failed:\n{log}");
    let mut mcp = seat.serve().await;
    mcp.ready().await;

    // From now on every stream is cut right after its first event; the person writes every half second.
    proxy.set(Knobs { cut_after: 1.0, ..Knobs::calm() });
    let before = proxy.counts().streams;
    let until = std::time::Instant::now() + Duration::from_secs(25);
    let mut n = 0;
    while std::time::Instant::now() < until {
        let _ = human.say(&group, json!({ "content_type": "message", "text": format!("note {n}") })).await;
        n += 1;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let opened = proxy.counts().streams - before;
    // About one a second and a half (a second's pause, up to half of it again by chance, and the event to wait
    // for). With a doubling pause it was 5: after 1, 2, 4, 8 seconds, then 16.
    assert!(opened >= 10, "{opened} streams in 25 s of cuts: the connector waits longer and longer");
    mcp.close().await;
}
