//! A really full account, held open for a first login to be traced (tests/client/mega/first-login.mjs).
//!
//! The room, written oldest first, as months of use leave it:
//! - `FULL_HUMANS` human devices (10): the stand-in that founded it and others let in by link, each with KeyPackages
//!   (every session founded after them has them as members);
//! - `FULL_AGENTS` agents (30), each its main session and `FULL_HELPERS` helper sessions (4): 150 sessions;
//! - the first agents' sessions are the old ones: written, then archived (helpers by their agent, main sessions at the
//!   hub); `FULL_LIVE_AGENTS` (10) agents stay, each with `FULL_LIVE_HELPERS` (2) live helpers: 30 live sessions;
//! - `FULL_CARDS` cards (12 000), nearly all closed by their agent, spread as a real account is: a live main session
//!   weighs 20, a live helper 3, an archived session 1 (so the live sessions hold about 70 %); per live session
//!   `FULL_OPEN` (2) open on the Desk, one "working" info (Off your mind) and `FULL_ANSWERED` (120 in all) answered
//!   and closed by the person;
//! - `FULL_CHAT` Chat lines (4 500), `FULL_PICTURES` cards with a picture (150, 20–60 KB each), `FULL_ARTIFACTS`
//!   Artifacts (50, each a file of 20–80 KB, in the live main sessions).
//!
//! Then it writes MEGA_OUT/full/fixture.json (hub, room, recovery code, the numbers) and holds: the live agents stay
//! online, the founding device answers key requests, until MEGA_OUT/full/release exists.
//!
//! ```text
//! cargo test -p trommi-tests --test client_mega full_account_held -- --ignored --nocapture
//! ```
use super::*;

fn num(name: &str, default: usize) -> usize {
    env(name).and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// Bytes that look like a picture to nobody: seeded noise.
fn noise(rng: &mut Rng, len: usize) -> Vec<u8> {
    (0..len).map(|_| (rng.next() >> 24) as u8).collect()
}

const WORDS: &[&str] = &[
    "build", "release", "hub", "web", "iPhone", "login", "cache", "desk", "card", "session", "agent", "design", "fix",
    "check", "deploy", "picture", "layout", "speed", "test", "server", "key", "change", "review", "branch",
];

fn words(rng: &mut Rng, n: usize) -> String {
    (0..n)
        .map(|_| *rng.pick(WORDS).unwrap())
        .collect::<Vec<_>>()
        .join(" ")
}

/** An upload, tried again while the room has its 16 uploads in progress (30 agents write at once). */
async fn upload(client: &Client, bytes: Vec<u8>, meta: Value) -> Map<String, Value> {
    for _ in 0..600 {
        match client.upload_attachment(bytes.clone(), fields(meta.clone())).await {
            Ok(r) => return r,
            Err(e) if e.code == "too-many" => tokio::time::sleep(Duration::from_millis(200)).await,
            Err(e) => panic!("an upload: {e:?}"),
        }
    }
    panic!("an upload: always too many in progress")
}

/** What one session holds. */
struct Quota {
    session: String,
    archived: bool,
    cards: usize,
    chat: usize,
    pictures: usize,
    artifacts: usize,
}

struct Plan {
    sessions: Vec<Quota>,
    open: usize,
    answered: usize,
    working: bool,
}

/// Writes one agent's sessions, oldest first; returns the cards left open for the person to answer.
async fn write_agent(client: Arc<Client>, plan: Plan, seed: u64) -> (Vec<String>, BTreeMap<String, usize>) {
    let mut rng = Rng(seed);
    let mut to_answer = Vec::new();
    let mut n: BTreeMap<String, usize> = BTreeMap::new();
    for q in &plan.sessions {
        let (session, archived) = (&q.session, &q.archived);
        let (cards, chat, pictures, artifacts) = (q.cards, q.chat, q.pictures, q.artifacts);
        for a in 0..artifacts {
            let len = 20_000 + rng.below(60_000);
            let bytes = noise(&mut rng, len);
            let reference = upload(&client, bytes, json!({ "name": format!("shot-{a}.png"), "media_type": "image/png" })).await;
            client
                .publish(
                    json!([reference]),
                    json!(format!("{} ({a})", words(&mut rng, 3))),
                    None,
                    Some(session.clone()),
                )
                .await
                .expect("an Artifact");
            *n.entry("artifacts".into()).or_default() += 1;
        }
        let open_here = if *archived { 0 } else { plan.open + plan.answered };
        for i in 0..cards {
            let mut card = json!({
                "card_type": if i % 3 == 0 { "info" } else { "decision" },
                "title": format!("{} #{i}", words(&mut rng, 5)),
                "body": format!("{}\n\n- {}\n- {}\n\n{}", words(&mut rng, 30), words(&mut rng, 8), words(&mut rng, 8), words(&mut rng, 20)),
                "urgency": "normal",
            });
            if i % 3 != 0 {
                card["options"] = json!([{ "key": "a", "label": "Do it", "detail": words(&mut rng, 4) }, { "key": "b", "label": "Later" }]);
                card["allows_multiple"] = json!(false);
                card["recommended"] = json!("a");
            }
            if i < pictures {
                let len = 20_000 + rng.below(40_000);
                let bytes = noise(&mut rng, len);
                let reference = upload(&client, bytes, json!({ "name": format!("pic-{i}.png"), "media_type": "image/png" })).await;
                card["attachments"] = json!([reference]);
                *n.entry("pictures".into()).or_default() += 1;
            }
            let last = i + open_here >= cards;
            if last {
                card["card_type"] = json!("decision");
                card["options"] = json!([{ "key": "a", "label": "Yes" }, { "key": "b", "label": "No" }]);
                card["allows_multiple"] = json!(false);
            }
            let id = client
                .send_card(fields(card), Some(session.clone()))
                .await
                .expect("a card");
            *n.entry("cards".into()).or_default() += 1;
            if last {
                if cards - i <= plan.answered {
                    to_answer.push(id);
                } else {
                    *n.entry("open".into()).or_default() += 1;
                }
            } else if i % 7 == 6 {
                client.withdraw(&id, "not needed any more").await.expect("withdrawn");
            } else {
                client.close(&id, &format!("Done: {}", words(&mut rng, 6))).await.expect("closed");
            }
            if i % 5 == 0 && chat > 0 {
                for _ in 0..(chat * 5 / cards.max(1)).max(1) {
                    let _ = client
                        .send_message(fields(json!({ "text": words(&mut rng, 12) })), None, Some(session.clone()))
                        .await;
                    *n.entry("chat".into()).or_default() += 1;
                }
            }
        }
        if !*archived && plan.working {
            client
                .send_card(
                    fields(json!({ "card_type": "info", "title": format!("Working on: {}", words(&mut rng, 4)), "body": "", "urgency": "normal", "working": true })),
                    Some(session.clone()),
                )
                .await
                .expect("a working card");
            *n.entry("working".into()).or_default() += 1;
        }
    }
    let _ = client.settle(120_000).await;
    (to_answer, n)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn full_account_held() {
    let humans_n = num("FULL_HUMANS", 10);
    let agents_n = num("FULL_AGENTS", 30);
    let helpers_n = num("FULL_HELPERS", 4);
    let live_agents = num("FULL_LIVE_AGENTS", 10).min(agents_n);
    let live_helpers = num("FULL_LIVE_HELPERS", 2).min(helpers_n);
    let cards_n = num("FULL_CARDS", 12_000);
    let chat_n = num("FULL_CHAT", 4_500);
    let pictures_n = num("FULL_PICTURES", 150);
    let artifacts_n = num("FULL_ARTIFACTS", 50);
    let open_n = num("FULL_OPEN", 2);
    let answered_n = num("FULL_ANSWERED", 120);
    let out = PathBuf::from(env("MEGA_OUT").unwrap_or_else(|| {
        std::env::temp_dir().join("trommi-mega-clients").display().to_string()
    }))
    .join("full");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let t0 = std::time::Instant::now();
    let log = |what: String| eprintln!("FULL {:>6.1}s {what}", t0.elapsed().as_secs_f64());

    let hub = start_hub().await;
    let code: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let keep = code.clone();
    let mut human = Human::found_with(&hub.url, move |_, c| {
        *keep.lock().unwrap() = Some(c.expose().to_vec());
        None
    })
    .await;
    let code_hex = trommi_connector::util::hex(&code.lock().unwrap().clone().unwrap());
    let room_hex = trommi_connector::util::hex(human.room.as_bytes());

    // the person's other devices (phone, laptops, old browsers): in the room before the sessions, so in every one
    let mut others = Vec::new();
    for _ in 1..humans_n {
        let mut h = human.add_human().await;
        human.follow_invites().await;
        h.publish_key_packages().await;
        others.push(h);
    }
    log(format!("{humans_n} human devices"));

    // the agents, each its main session and its helpers; the sessions of agent i are the i-th oldest
    let mut agents = Vec::new();
    for i in 0..agents_n {
        let (client, drain, dir, admitted) = join_agent(&mut human).await;
        let group = human.found_session(&admitted).await;
        let main = trommi_connector::util::hex(group.session_id().unwrap().as_bytes());
        let a = client.clone();
        common::eventually("the agent is in its session", || async { a.core.lock().await.session_id().is_some() }).await;
        let _ = client
            .set_status(fields(json!({ "profile": { "agent_name": format!("project-{i}") } })), Some(main.clone()))
            .await;
        let mut helpers = Vec::new();
        for h in 0..helpers_n {
            let id = client
                .open_child_session(fields(json!({ "agent_name": format!("helper {h} of project-{i}") })))
                .await
                .expect("a helper session");
            helpers.push(id);
        }
        agents.push((client, drain, dir, group, main, helpers));
    }
    log(format!("{agents_n} agents, {} sessions", agents_n * (1 + helpers_n)));

    // what each session holds: the archived ones (old agents; the first helpers of the live agents) first
    let sessions_total = agents_n * (1 + helpers_n);
    let live_from = agents_n - live_agents;
    let mut jobs = Vec::new();
    let live_sessions = live_agents * (1 + live_helpers);
    let answered_per_session = answered_n / live_sessions.max(1);
    // How a real account is spread: the live main sessions run for months and hold most of it (weight 20), a live
    // helper some (3), an archived session a little (1); the Artifacts sit in the live main sessions.
    let weight = |archived: bool, main: bool| if archived { 1 } else if main { 20 } else { 3 };
    let weights: usize = (0..agents_n)
        .map(|i| {
            let live = i >= live_from;
            weight(!live, true) + (0..helpers_n).map(|h| weight(!(live && h >= helpers_n - live_helpers), false)).sum::<usize>()
        })
        .sum();
    for (i, (client, _, _, _, main, helpers)) in agents.iter().enumerate() {
        let live = i >= live_from;
        let mut sessions: Vec<(String, bool, bool)> = helpers
            .iter()
            .enumerate()
            .map(|(h, s)| (s.clone(), !(live && h >= helpers_n - live_helpers), false))
            .collect();
        sessions.sort_by_key(|(_, archived, _)| !*archived);
        sessions.push((main.clone(), !live, true));
        let sessions = sessions
            .into_iter()
            .map(|(session, archived, is_main)| {
                let w = weight(archived, is_main);
                let share = |total: usize| total * w / weights;
                Quota {
                    session,
                    archived,
                    cards: share(cards_n),
                    chat: share(chat_n),
                    pictures: share(pictures_n),
                    artifacts: if is_main && live { artifacts_n / live_agents.max(1) } else { 0 },
                }
            })
            .collect();
        let _ = sessions_total;
        let plan = Plan {
            sessions,
            open: open_n,
            answered: answered_per_session,
            working: true,
        };
        jobs.push(tokio::spawn(write_agent(client.clone(), plan, 1000 + i as u64)));
    }
    let mut to_answer = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for j in jobs {
        let (ids, n) = j.await.expect("an agent's writing");
        to_answer.extend(ids);
        for (k, v) in n {
            *counts.entry(k).or_default() += v;
        }
    }
    log(format!("written: {counts:?}"));

    // archived: the helpers by their agent, the old main sessions at the hub
    let mut archived = 0;
    for (i, (client, _, _, group, _, helpers)) in agents.iter().enumerate() {
        let live = i >= live_from;
        for (h, s) in helpers.iter().enumerate() {
            if !(live && h >= helpers_n - live_helpers) {
                client.archive_child_session(s).await.expect("a helper archived");
                archived += 1;
            }
        }
        if !live {
            human.archive(group).await;
            archived += 1;
        }
    }
    log(format!("{archived} sessions archived"));

    // the person answers some, each closing its card
    human.sync().await;
    let known = human.object_ids();
    let mut answers = 0;
    for id in to_answer.iter().filter(|id| known.contains(*id)) {
        if human
            .answer(id, json!({ "schema_version": 2, "answer_action": "answer", "choices": ["a"] }), &["a"], true)
            .await
            .is_ok()
        {
            answers += 1;
        }
    }
    for (client, ..) in &agents {
        let _ = client.settle(60_000).await;
    }
    log(format!("{answers} answered"));
    // the old agents are gone (their sessions archived); the live ones stay online
    for (i, (client, ..)) in agents.iter().enumerate() {
        if i < live_from {
            client.stop().await;
        }
    }
    human.sync().await;
    let head = human.hub.get("/v1/changes?after=0&limit=1").await.ok();
    counts.insert("humans".into(), humans_n);
    counts.insert("agents".into(), agents_n);
    counts.insert("sessions".into(), sessions_total);
    counts.insert("archived".into(), archived);
    counts.insert("live_sessions".into(), live_sessions);
    counts.insert("answered".into(), answers);
    let fixture = json!({
        "hub": hub.url, "room_id": room_hex, "code": code_hex, "counts": counts,
        "head": head.and_then(|h| h.get("head").cloned()), "built_s": t0.elapsed().as_secs(),
    });
    std::fs::write(out.join("fixture.json"), serde_json::to_string_pretty(&fixture).unwrap()).unwrap();
    log(format!("HELD {fixture}"));

    // held: the founding device keeps up and answers key requests until released
    let release = out.join("release");
    while !release.exists() {
        human.sync().await;
        let _ = human.answer_key_requests().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    log("released".into());
    for (client, ..) in &agents {
        client.stop().await;
    }
    drop(others);
    drop(hub);
}
