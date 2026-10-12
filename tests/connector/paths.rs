//! The user's paths end to end, on a local hub with the real binary: what a session that cannot go on says,
//! and that the way it names gets it onto the board again.
mod common;

use common::process::Seat;
use common::{HubProc, Human, TempDir};
use serde_json::{json, Value};

fn tool_names(listed: &Value) -> Vec<String> {
    listed["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect()
}

/// A folder joined by the CLI, with its human.
async fn joined_on(hub: &HubProc) -> (Human, Seat) {
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let join = seat.join(&link);
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((joined, log), _group) = tokio::join!(join, admit);
    assert!(joined, "the join failed:\n{log}");
    (human, seat)
}

/// The hub was set up again (its data gone): the folder's key is of a room no hub knows. The session says so,
/// and the connect tool with a new link of the new room gets it onto the board, without a restart.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_of_a_room_the_hub_no_longer_knows_gives_way_to_a_new_invite() {
    let hub = HubProc::start().await;
    let (_old_human, seat) = joined_on(&hub).await;
    let (_data, port) = hub.stop();
    let hub = HubProc::start_in(TempDir::new("hub"), port).await;
    let mut human = Human::found(&hub.url).await;

    let mut mcp = seat.serve().await;
    // the session learns that its key is out, says how to get back, and offers connect for that
    let mut said = String::new();
    for _ in 0..100 {
        let (text, failed) = mcp.call("list_cards", json!({})).await;
        assert!(failed, "{text}");
        said = text;
        if said.contains("/trommi:connect") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(said.contains("/trommi:connect"), "{said}");
    let listed = mcp.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");

    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), _group) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    mcp.ready().await;
    let listed = mcp.request("tools/list", json!({})).await;
    assert!(tool_names(&listed).iter().any(|n| n == "reply"), "{listed}");
    mcp.close().await;

    // after a restart the folder goes on in the new room
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    mcp.close().await;
}

/// What a hint tells the human to run exists: the connector is updated by `trommi-connector update` (the plugin
/// holds no program, and there is no connect script any more), and the join command is `connect`.
#[tokio::test(flavor = "multi_thread")]
async fn the_update_and_usage_hints_name_commands_that_exist() {
    let line = trommi_connector::host::update_line();
    assert!(line.contains("trommi-connector update"), "{line}");
    assert!(!line.contains("connect script") && !line.contains("plugin"), "{line}");

    let seat = Seat::new("http://127.0.0.1:9");
    let output = seat
        .command(&["connect"])
        .env_remove("TROMMI_INVITE")
        .output()
        .await
        .expect("connect runs");
    let told = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{told}");
    assert!(told.contains("trommi-connector connect '<invite link>'"), "{told}");
}

/// The update card is filed in the human's language: no German words are put into the agent's mouth.
#[test]
fn the_update_card_and_its_tool_are_not_in_german() {
    let mut texts = vec![trommi_connector::prompt::instructions()];
    texts.push(trommi_connector::prompt::tools_list_json(true));
    for text in texts {
        for word in ["jetzt", "später", "Neue Connector"] {
            assert!(!text.contains(word), "{word} in: {text}");
        }
    }
}
