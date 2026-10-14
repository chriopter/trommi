//! The past on demand (v1.md 9.0.6, hub-api.md point 47): a sign-in loads only the open cards and what is new, so
//! older history is read from the hub when a page needs it, out of the hub's order and checked by the core like any
//! envelope ([`Engine::show_items`]):
//!
//! - **a session opened**: the newest page of its Chat (`GET /v1/chats/session/{id}/items`), once a run; "Earlier
//!   messages" (`Action::ShowMore`) reads the page before the oldest one read, while the hub says it has more. Until
//!   the newest page came the session's page shows no welcome ("Joined and waiting for its first task") — that is
//!   only for a session whose Chat the hub holds nothing of (`Lens::talk_waits`);
//! - **a card opened**: the newest page of its Chat, and every version the hub holds of it (`GET /v1/cards/{id}`,
//!   at most [`VERSIONS_MAX`]), once a run;
//! - **the page Off your mind opened**: the newest page of the settled cards (`GET /v1/objects?state=settled`,
//!   [`SETTLED_PAGE`] a page), each one's current version at once, then the envelopes of each (its answer: else an
//!   answered card stands as open, a closed one as withdrawn), a few at a time; "Show more" reads the page below, while the hub says it has more;
//! - **a starred card this device does not hold** (a star is a room register, which the Desk brings; the card may
//!   be long closed): its versions, once a run, a few at a time, so the starred stack stands whole.
//!
//! Each read is asked once a run: what failed or was refused is not asked again until the next run (or, for a
//! page of a Chat, until the person asks for it again), so nothing here polls.

use crate::app::io::{HttpResponse, IoId};
use crate::engine::{Ctx, Engine};
use crate::hub::{Answer, Codec, Expect, Failure, ObjectRoute, Route, Timeline};
use crate::model::Model;
use crate::views::Lens;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use trommi_core::ids::{ObjectId, SessionId};

/// Items of a Chat in one page read from the hub.
pub const CHAT_PAGE: u32 = 50;
/// Versions of a card asked in one request.
const VERSIONS_PAGE: u32 = 100;
/// At most so many versions of one card are read.
pub const VERSIONS_MAX: usize = 1000;
/// Starred cards whose versions are read at once.
const STARRED_AT_ONCE: usize = 4;
/// Settled cards in one page of the list Off your mind.
pub const SETTLED_PAGE: u32 = 100;
/// Settled cards whose envelopes are read at once.
const HEADS_AT_ONCE: usize = 6;

enum Asked {
    Chat { key: String, expect: Expect },
    /// `starred`: read for the starred stack (counted in `starred_flying`).
    Versions { card: String, read: usize, starred: bool, expect: Expect },
    Settled { expect: Expect },
    /// A settled card's envelopes from its first version on (its answer among them): one page.
    Head { expect: Expect },
}

/// The settled cards read for the page Off your mind.
#[derive(Default)]
struct Settled {
    /// The first page was asked this run.
    asked: bool,
    flying: bool,
    /// The hub holds more below this change.
    below: Option<u64>,
}

/// A timeline's window as a page left it: the oldest change read, and whether the hub holds more before it.
pub struct Window {
    pub key: String,
    pub down_to: Option<u64>,
    pub more: bool,
}

#[derive(Default)]
pub struct Past {
    asked: BTreeMap<IoId, Asked>,
    /// Timelines with a page on its way.
    flying: BTreeSet<String>,
    /// Timelines whose newest page came (or was refused) this run.
    read: BTreeSet<String>,
    /// Cards whose versions are read or were read this run.
    versions: BTreeSet<String>,
    /// Starred cards whose versions are on their way.
    starred_flying: usize,
    /// The windows the pages answered since the App last took them (`take_windows`).
    windows: Vec<Window>,
    /// The session the page in view names, as last found: (the page's id, the session's key, its session id).
    found: Option<(String, String, String)>,
    settled: Settled,
    /// Settled cards whose envelopes are still to read, and how many are on their way.
    heads: std::collections::VecDeque<(ObjectId, u64)>,
    heads_flying: usize,
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn ask(engine: &mut Engine, ctx: &mut Ctx<'_>, route: Route<'_>) -> Option<(IoId, Expect)> {
    let cell: Rc<RefCell<Option<Expect>>> = Rc::new(RefCell::new(None));
    let put = cell.clone();
    let id = engine
        .request(
            ctx,
            move |site| {
                let (request, expect) = Codec.request(site, &route)?;
                *put.borrow_mut() = Some(expect);
                Ok(request)
            },
            None,
        )
        .ok()?;
    let expect = cell.borrow_mut().take()?;
    Some((id, expect))
}

/// The session a page names (its key, its short id, or its session's id): its key and its session's id.
pub fn session_of(model: &Model, id: &str) -> Option<(String, String)> {
    crate::sessions::session_list(model, false)
        .into_iter()
        .find(|r| r.key == id || r.id == id || r.session_id.as_deref() == Some(id))
        .and_then(|r| Some((r.key.clone(), r.session_id.clone()?)))
}

/// The timeline key of a session's Chat.
pub fn talk_key(session_key: &str) -> String {
    format!("chat:session/{session_key}")
}

impl Past {
    pub fn owns(&self, id: IoId) -> bool {
        self.asked.contains_key(&id)
    }

    /// The windows set by pages since the last call.
    pub fn take_windows(&mut self) -> Vec<Window> {
        std::mem::take(&mut self.windows)
    }

    /// Whether the newest page of this timeline is still to come this run.
    pub fn waits(&self, key: &str) -> bool {
        !self.read.contains(key)
    }

    /// The session a page names (`session_of`), remembered for the page in view: its key and its session id.
    pub fn session(&mut self, model: &Model, id: &str) -> Option<(String, String)> {
        if let Some((of, key, session)) = &self.found {
            if of == id {
                return Some((key.clone(), session.clone()));
            }
        }
        let (key, session) = session_of(model, id)?;
        self.found = Some((id.to_string(), key.clone(), session.clone()));
        Some((key, session))
    }

    /// The session in view whose newest page of history has not come yet: its key.
    pub fn talk_waits(&mut self, lens: &Lens, model: &Model) -> Option<String> {
        let (key, _) = self.session(model, lens.session.as_deref()?)?;
        self.waits(&talk_key(&key)).then_some(key)
    }

    /// Asks what the pages in view and the starred stack need and this run has not asked yet.
    pub fn tend(&mut self, lens: &Lens, model: &Model, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        if let Some((key, session)) = lens.session.as_deref().and_then(|id| self.session(model, id)) {
            let key = talk_key(&key);
            if !self.read.contains(&key) {
                if let Some(session) = unhex(&session).and_then(|b| SessionId::from_slice(&b).ok()) {
                    self.chat(Timeline::Session(&session), key, None, engine, ctx);
                }
            }
        }
        if let Some(card) = lens.card.as_deref().and_then(|id| card_of(model, id)) {
            let key = format!("chat:card/{card}");
            if let Some(object) = unhex(&card).and_then(|b| ObjectId::from_slice(&b).ok()) {
                if !self.read.contains(&key) {
                    self.chat(Timeline::Card(&object), key, None, engine, ctx);
                }
                if self.versions.insert(card.clone()) {
                    self.versions_from(card, object, 0, 0, false, engine, ctx);
                }
            }
        }
        // the page Off your mind: its newest page of settled cards
        if lens.off.is_some() && !self.settled.asked {
            self.settled.asked = true;
            self.settled_page(None, engine, ctx);
        }
        self.tend_heads(engine, ctx);
        // the starred cards this device does not hold
        if self.starred_flying < STARRED_AT_ONCE {
            let starred: Vec<String> = model
                .human
                .range::<str, _>((std::ops::Bound::Included("star/"), std::ops::Bound::Excluded("star0")))
                .filter(|(_, r)| r.value.get("at").is_some())
                .filter_map(|(k, _)| k.strip_prefix("star/"))
                .filter(|id| !model.cards.contains_key(*id) && !self.versions.contains(*id))
                .map(str::to_string)
                .take(STARRED_AT_ONCE - self.starred_flying)
                .collect();
            for card in starred {
                self.versions.insert(card.clone());
                let Some(object) = unhex(&card).and_then(|b| ObjectId::from_slice(&b).ok()) else { continue };
                if self.versions_from(card, object, 0, 0, true, engine, ctx) {
                    self.starred_flying += 1;
                }
            }
        }
    }

    /// Whether a page of the settled cards is on its way, and whether the hub holds more below the ones read.
    pub fn settled_state(&self) -> (bool, bool) {
        (self.settled.flying || (self.settled.asked && self.heads_flying > 0), self.settled.below.is_some())
    }

    /// "Show more" on the page Off your mind, past what this device holds: the hub's next page.
    pub fn more_settled(&mut self, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        if let (Some(below), false) = (self.settled.below, self.settled.flying) {
            self.settled_page(Some(below), engine, ctx);
        }
    }

    fn settled_page(&mut self, before: Option<u64>, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let route = Route::Settled { kind: Some(ObjectRoute::Cards), only: crate::hub::Only::Active, before, limit: SETTLED_PAGE };
        if let Some((id, expect)) = ask(engine, ctx, route) {
            self.settled.flying = true;
            self.asked.insert(id, Asked::Settled { expect });
        }
    }

    fn tend_heads(&mut self, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        while self.heads_flying < HEADS_AT_ONCE {
            let Some((object, after)) = self.heads.pop_front() else { break };
            if let Some((id, expect)) = ask(engine, ctx, Route::Object { route: ObjectRoute::Cards, object: &object, after, limit: 20 }) {
                self.heads_flying += 1;
                self.asked.insert(id, Asked::Head { expect });
            }
        }
    }

    /// "Earlier messages": the page before the oldest one read, when the hub holds more.
    pub fn earlier(&mut self, session_id: &str, model: &Model, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some((key, session)) = session_of(model, session_id) else { return };
        let key = talk_key(&key);
        let Some(t) = model.timelines.get(&key) else { return };
        if !t.has_more || !self.read.contains(&key) {
            return;
        }
        let before = t.loaded_down_to;
        if let Some(session) = unhex(&session).and_then(|b| SessionId::from_slice(&b).ok()) {
            self.chat(Timeline::Session(&session), key, before, engine, ctx);
        }
    }

    /// "Load earlier" under a card: the page of its Chat before the oldest one read.
    pub fn earlier_comments(&mut self, card: &str, model: &Model, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some(card) = card_of(model, card) else { return };
        let key = format!("chat:card/{card}");
        let Some(t) = model.timelines.get(&key) else { return };
        if !t.has_more || !self.read.contains(&key) {
            return;
        }
        let before = t.loaded_down_to;
        if let Some(object) = unhex(&card).and_then(|b| ObjectId::from_slice(&b).ok()) {
            self.chat(Timeline::Card(&object), key, before, engine, ctx);
        }
    }

    fn chat(&mut self, timeline: Timeline<'_>, key: String, before: Option<u64>, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        if self.flying.contains(&key) {
            return;
        }
        if let Some((id, expect)) = ask(engine, ctx, Route::Chat { timeline, before, limit: CHAT_PAGE }) {
            self.flying.insert(key.clone());
            self.asked.insert(id, Asked::Chat { key, expect });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn versions_from(&mut self, card: String, object: ObjectId, after: u64, read: usize, starred: bool, engine: &mut Engine, ctx: &mut Ctx<'_>) -> bool {
        match ask(engine, ctx, Route::Object { route: ObjectRoute::Cards, object: &object, after, limit: VERSIONS_PAGE }) {
            Some((id, expect)) => {
                self.asked.insert(id, Asked::Versions { card, read, starred, expect });
                true
            }
            None => false,
        }
    }

    /// The hub's answer to one of these reads: its envelopes go to the core, read out of the hub's order.
    pub fn answered(&mut self, id: IoId, result: &Result<HttpResponse, Failure>, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        let answer = |expect: &Expect| match result {
            Ok(response) => Codec.answer(expect, response),
            Err(failure) => Err(failure.clone()),
        };
        match asked {
            Asked::Chat { key, expect } => {
                self.flying.remove(&key);
                self.read.insert(key.clone());
                match answer(&expect) {
                    Ok(Answer::Envelopes { items, more }) => {
                        engine.show_items(ctx, &items);
                        let down_to = items.iter().map(|i| i.change).min();
                        self.windows.push(Window { key, down_to, more: more && !items.is_empty() });
                    }
                    // (refused, or not read: nothing more is asked of this Chat in this run but by the person)
                    _ => self.windows.push(Window { key, down_to: None, more: false }),
                }
            }
            Asked::Settled { expect } => {
                self.settled.flying = false;
                self.settled.below = None;
                if let Ok(Answer::Settled(page)) = answer(&expect) {
                    let mut versions: Vec<_> = page.items.iter().filter_map(|o| o.row.version.clone()).collect();
                    versions.sort_by_key(|v| v.change);
                    engine.show_items(ctx, &versions);
                    // (its answer is read too, which lies between its first version and its last: else an answered
                    //  card stands as open, and a closed one as withdrawn; one card's envelopes in one request)
                    for o in page.items.iter().filter(|o| o.row.head_change > o.row.first_change) {
                        self.heads.push_back((o.row.object, o.row.first_change.saturating_sub(1)));
                    }
                    self.settled.below = page.more.then_some(page.before).filter(|b| *b > 0);
                    self.tend_heads(engine, ctx);
                }
            }
            Asked::Head { expect } => {
                self.heads_flying = self.heads_flying.saturating_sub(1);
                if let Ok(Answer::Object(got)) = answer(&expect) {
                    engine.show_items(ctx, &got.items);
                }
                self.tend_heads(engine, ctx);
            }
            Asked::Versions { card, read, starred, expect } => {
                match answer(&expect) {
                    Ok(Answer::Object(got)) => {
                        engine.show_items(ctx, &got.items);
                        let read = read + got.items.len();
                        if let (true, Some(last), true) = (got.more, got.items.last().map(|i| i.change), read < VERSIONS_MAX) {
                            if self.versions_from(card, got.object, last, read, starred, engine, ctx) {
                                return;
                            }
                        }
                    }
                    _ => {}
                }
                if starred {
                    self.starred_flying = self.starred_flying.saturating_sub(1);
                }
            }
        }
    }
}

/// The card a page names (its id, or its number on the board when this device holds it), as 32 hex digits.
fn card_of(model: &Model, id: &str) -> Option<String> {
    if model.cards.contains_key(id) {
        return Some(id.to_string());
    }
    (id.len() == 32 && unhex(id).is_some()).then(|| id.to_string())
}
