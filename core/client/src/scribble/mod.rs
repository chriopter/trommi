//! The Scribble Board (spec/v1.md 10), written once for every app: which board is in view, reading it (its newest
//! snapshot, then the items after it), what its items add up to (the protocol core's `board_items::Board`), the
//! shapes as the shell draws them (`views::BoardView`), the strokes others are still drawing (7.2), and the snapshots
//! this device writes so that the hub can prune the board's items (10.9). The web's `CanvasState`, `openCanvas`,
//! `loadBoard` and `writeSnapshot` of before.
//!
//! - **Reading**: when a board comes into view (`Focus.board`, `Screen::Scribble`) and the room is live, the
//!   register `scribble_snapshot/desk/<board>` names the newest snapshot. Its file is fetched, decrypted, unpacked
//!   (`gzip`) and loaded (`Board::from_snapshot`, with the register's frontier); then the board's items are read in
//!   pages (`GET /v1/boards/{board}`) from `items_after_change()` on, each item read by the engine out of the hub's
//!   order as a Desk's version is. A snapshot that does not load is dropped and the board read from its first item.
//!   What arrives meanwhile (live items, the pages) is held and applied in the hub's order once the last page came.
//! - **Live**: a board item the engine reports is applied at once to every board that is read; a stroke piece grows
//!   the stroke shown under `live:<sender>/<stroke>` until its finished form arrives (or nothing came for a while).
//! - **Own items**: `Action::BoardItem` echoes into the model's timeline `scribble:desk/<board>` (`actions::run`);
//!   the view lays those echoes over the board until the hub's copy is applied, and a refusal takes them back.
//! - **Snapshots**: after [`SNAP_EVERY`] items and [`SNAP_IDLE_MS`] without one, with nothing of this device in
//!   flight for the board and every item read: the frontier is declared (`POST /v1/boards/{board}/frontier`), the
//!   gzip'd file uploaded encrypted, the register written, and once the hub took the register the frontier is bound
//!   to it. [`Scribble::snapshot_now`] writes one at once (a desk's delete: the empty board).

pub mod gzip;

use crate::app::io::{HttpRequest, IoId};
use crate::desk_delete::BoardKnown;
use crate::engine::{Ctx, Engine};
use crate::hub::{Answer, Codec, Expect, Failure, Route, Site};
use crate::model::Model;
use crate::views::{BoardShape, BoardView};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use trommi_core::board::{ShapeId, ITEMS_BEFORE_SNAPSHOT};
use trommi_core::board_items::{Applied, Board, Ink, ItemBody, Shape, ShapeKind, Unread, MAX_SNAPSHOT_LEN, QUANTUM};
use trommi_core::chain::Head;
use trommi_core::crypto::SecretBytes;
use trommi_core::device::{Draft, EnvelopeOutcome, ReceivedEnvelope};
use trommi_core::envelope::{Subject, Timeline};
use trommi_core::files::FileRef;
use trommi_core::ids::{BoardId, DeviceId, FileId, GroupId, Hash32};
use trommi_core::Error;

/// A board writes a snapshot after so many items applied since the last one.
pub const SNAP_EVERY: u64 = 300;
/// … and once this long has passed without an item.
pub const SNAP_IDLE_MS: u64 = 4_000;
/// Items asked for in one page.
pub const PAGE: u32 = 500;
/// Shapes erased in one item (an item names at most 2 000).
pub const ERASE_CHUNK: usize = 400;
/// A stroke someone is drawing goes when no piece of it came for this long.
pub const LIVE_MS: u64 = 30_000;
/// The snapshot file's name and type in its reference.
pub const SNAPSHOT_FILE: &str = "canvas.json.gz";
pub const SNAPSHOT_TYPE: &str = "application/gzip";

fn hex(bytes: &[u8]) -> String {
    crate::engine::feed::hex(bytes)
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

/// The board a model id names: `desk/<32 hex>`.
pub fn board_of(id: &str) -> Option<BoardId> {
    let hex = id.strip_prefix("desk/")?;
    if hex.len() != 32 || hex.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }
    BoardId::from_slice(&unhex(hex)?).ok()
}

/// The board of a desk (10.1): a desk id of 32 hex is its board's id; "all" is the board of All desks; any other
/// name (`main`, a room without desks) is folded into 16 bytes, its UTF-8 XORed by position and the length XORed
/// onto the last byte, so every client finds the same board.
pub fn desk_board(desk: &str) -> String {
    if desk == crate::views::board::ALL_DESKS {
        return format!("desk/{}", hex(BoardId::ALL_DESKS.as_bytes()));
    }
    if desk.len() == 32 && desk.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return format!("desk/{desk}");
    }
    let name = if desk.is_empty() { "main" } else { desk };
    let mut out = [0u8; 16];
    for (i, b) in name.bytes().enumerate() {
        out[i % 16] ^= b;
    }
    out[15] ^= (name.len() & 0xff) as u8;
    format!("desk/{}", hex(&out))
}

/// The board a focus names: a board id as it is, or the board of a desk.
pub fn board_in_view(text: &str) -> Option<BoardId> {
    board_of(text).or_else(|| board_of(&desk_board(text)))
}

/// The model's id of a shape: `<sender hex>/<number>/<index>`.
pub fn shape_key(id: &ShapeId) -> String {
    format!("{}/{}/{}", hex(id.sender.as_bytes()), id.seq, id.index)
}

fn units(q: i32) -> f64 {
    f64::from(q) / f64::from(QUANTUM)
}

/// A force byte as the web's `unpackPoints` reads it: three decimals.
fn force(b: u8) -> f64 {
    (f64::from(b) / 255.0 * 1000.0).round() / 1000.0
}

/// The points of `ink` added to a shape's lists.
fn put_ink(s: &mut BoardShape, ink: &Ink) {
    let tilted = ink.tilted();
    for p in ink.points() {
        s.pts.push(units(p.x));
        s.pts.push(units(p.y));
        s.t.push(u64::from(p.t));
        s.f.push(force(p.force));
        if tilted {
            s.az.get_or_insert_with(Vec::new).push(f64::from(p.azimuth) / 256.0 * std::f64::consts::TAU);
            s.al.get_or_insert_with(Vec::new).push(f64::from(p.altitude) / 255.0 * std::f64::consts::FRAC_PI_2);
        }
    }
    s.sim |= ink.simulated();
}

/// The reference (model spelling, with its key) of a file the protocol core names.
fn reference_of(file: &FileRef, name: &str, media_type: &str, size: u64, width: Option<u32>, height: Option<u32>) -> Map<String, Value> {
    let mut r = Map::new();
    r.insert("attachment_id".into(), json!(hex(file.file_id.as_bytes())));
    r.insert("file_key".into(), json!(trommi_core::ids::base64url_encode(file.file_key.expose())));
    r.insert("sha256".into(), json!(file.sha256.to_base64url()));
    r.insert("file_name".into(), json!(name));
    r.insert("media_type".into(), json!(media_type));
    r.insert("total_size".into(), json!(size));
    if let Some(w) = width {
        r.insert("width".into(), json!(w));
    }
    if let Some(h) = height {
        r.insert("height".into(), json!(h));
    }
    r
}

/// The protocol core's reference of a model one.
fn file_ref(reference: &Map<String, Value>) -> Option<FileRef> {
    let id = FileId::from_slice(&unhex(reference.get("attachment_id")?.as_str()?)?).ok()?;
    FileRef::from_base64url(&id.to_base64url(), reference.get("file_key")?.as_str()?, reference.get("sha256")?.as_str()?).ok()
}

/// One shape as the shell draws it, and the reference of its picture.
fn view_of(id: String, by: String, shape: &Shape) -> (BoardShape, Option<Map<String, Value>>) {
    let mut s = BoardShape { id, by, z: shape.z, group: shape.group.clone(), ..Default::default() };
    let mut reference = None;
    match &shape.kind {
        ShapeKind::Stroke(stroke) => {
            s.tool = match stroke.pen {
                trommi_core::board_items::Pen::Pen => "pen",
                trommi_core::board_items::Pen::Marker => "marker",
            }
            .into();
            put_ink(&mut s, &stroke.ink);
            s.color = Some(stroke.color.clone());
            s.width = Some(units(stroke.width));
        }
        ShapeKind::Note(note) => {
            s.tool = match note.kind {
                trommi_core::board_items::NoteKind::Text => "text",
                trommi_core::board_items::NoteKind::Sticky => "sticky",
                trommi_core::board_items::NoteKind::Voice => "voice",
            }
            .into();
            s.pts = vec![units(note.at[0]), units(note.at[1])];
            s.text = Some(note.text.clone());
            s.size = Some(units(note.size));
            s.color = Some(note.color.clone());
            s.wrap = note.wrap.map(units);
        }
        ShapeKind::Picture(p) => {
            s.tool = "image".into();
            s.pts = p.rect.iter().map(|q| units(*q)).collect();
            s.attachment = Some(hex(p.file.file_id.as_bytes()));
            s.nw = Some(p.width.unwrap_or(0));
            s.nh = Some(p.height.unwrap_or(0));
            s.mime = Some(p.media_type.clone());
            s.name = Some(p.file_name.clone());
            reference = Some(reference_of(&p.file, &p.file_name, &p.media_type, p.total_size, p.width, p.height));
        }
    }
    (s, reference)
}

/// An item of a board as the engine reported it, held while the board is read.
struct Held {
    sender: DeviceId,
    seq: u64,
    hash: Hash32,
    got: Got,
}

enum Got {
    Body(SecretBytes),
    Unread(Unread),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// In view, waiting for the room to be live.
    Waiting,
    /// The snapshot's file is fetched.
    Snapshot,
    /// The items after the snapshot are read.
    Tail,
    Ready,
}

/// A snapshot this device writes, step by step.
struct Snapping {
    frontier: Vec<(DeviceId, Head)>,
    files: Vec<FileId>,
    /// The items it covers of those counted since the last one.
    covers: u64,
    newest_change: u64,
    empty: bool,
    /// The register's outbox entry, once sealed, with its number and hash.
    register: Option<(u64, u64, Hash32)>,
}

/// One board this device reads.
struct Pad {
    id: BoardId,
    step: Step,
    board: Board,
    /// Per writer, the last item taken into account: what a snapshot's frontier names.
    heads: BTreeMap<DeviceId, Head>,
    /// The shapes as the shell draws them, by id (ascending: the order they were made).
    shapes: BTreeMap<ShapeId, BoardShape>,
    /// The references of its pictures, by attachment id.
    refs: BTreeMap<String, Map<String, Value>>,
    /// Strokes others are drawing, by `live:<sender>/<stroke>`, with when their last piece came.
    live: BTreeMap<String, (BoardShape, u64)>,
    /// Items that came while the board is read, by change.
    held: BTreeMap<u64, Held>,
    /// A page is asked for; the last page came.
    paging: bool,
    tail_done: bool,
    /// The newest change number seen of the board.
    newest_change: u64,
    /// Items applied since the last snapshot, and when the last one was.
    applied: u64,
    last_at: u64,
    snapping: Option<Snapping>,
    /// A snapshot is wanted at once (`snapshot_now`).
    force: bool,
    /// The newest snapshot this device wrote or loaded holds no shape and covers every item.
    snapshot_empty: bool,
}

impl Pad {
    fn new(id: BoardId) -> Pad {
        Pad {
            id,
            step: Step::Waiting,
            board: Board::new(),
            heads: BTreeMap::new(),
            shapes: BTreeMap::new(),
            refs: BTreeMap::new(),
            live: BTreeMap::new(),
            held: BTreeMap::new(),
            paging: false,
            tail_done: false,
            newest_change: 0,
            applied: 0,
            last_at: 0,
            snapping: None,
            force: false,
            snapshot_empty: false,
        }
    }

    fn hex(&self) -> String {
        hex(self.id.as_bytes())
    }

    /// Starts over from `board` (empty, or a snapshot's) with these heads.
    fn reset(&mut self, board: Board, heads: &[(DeviceId, Head)]) {
        self.heads = heads.iter().copied().collect();
        self.shapes.clear();
        self.refs.clear();
        for (id, shape) in board.shapes() {
            let (view, reference) = view_of(shape_key(id), hex(id.sender.as_bytes()), shape);
            if let (Some(r), Some(a)) = (reference, view.attachment.clone()) {
                self.refs.insert(a, r);
            }
            self.shapes.insert(*id, view);
        }
        self.board = board;
        self.applied = 0;
    }

    fn head(&mut self, sender: DeviceId, seq: u64, hash: Hash32) {
        if self.heads.get(&sender).is_none_or(|h| h.seq < seq) {
            self.heads.insert(sender, Head { seq, hash });
        }
    }

    /// One item into the board.
    fn apply(&mut self, item: Held, now: u64) {
        let Held { sender, seq, hash, got } = item;
        if seq <= self.board.applied(&sender) {
            return;
        }
        match got {
            Got::Body(payload) => match ItemBody::decode(payload.expose()) {
                Ok(body) => {
                    let mut finished = Vec::new();
                    if let ItemBody::Strokes(shapes) = &body {
                        for shape in shapes {
                            if let ShapeKind::Stroke(s) = &shape.kind {
                                if let Some(live) = s.live {
                                    finished.push(format!("live:{}/{}", hex(sender.as_bytes()), hex(&live)));
                                }
                            }
                        }
                    }
                    let Ok(Applied::Changed(ids)) = self.board.apply(sender, seq, body) else { return };
                    for id in ids {
                        match self.board.shape(&id) {
                            Some(shape) => {
                                let (view, reference) = view_of(shape_key(&id), hex(id.sender.as_bytes()), shape);
                                if let (Some(r), Some(a)) = (reference, view.attachment.clone()) {
                                    self.refs.insert(a, r);
                                }
                                self.shapes.insert(id, view);
                            }
                            None => {
                                self.shapes.remove(&id);
                            }
                        }
                    }
                    for live in finished {
                        self.live.remove(&live);
                    }
                }
                Err(Error::NewerVersion) => self.board.skip_unread(sender, seq, Unread::Newer),
                // (a body the core refuses adds nothing, and the writer's number stays where it was)
                Err(_) => return,
            },
            Got::Unread(why) => self.board.skip_unread(sender, seq, why),
        }
        self.head(sender, seq, hash);
        self.applied += 1;
        self.last_at = now;
        self.snapshot_empty = false;
    }

    fn state(&self) -> &'static str {
        match (self.step, self.board.unread()) {
            (Step::Ready, None) => "ready",
            (Step::Ready, Some(Unread::Newer)) => "newer_version",
            (Step::Ready, Some(Unread::NoKey)) => "withheld",
            _ => "loading",
        }
    }
}

/// What a request in flight was for.
enum Asked {
    Snapshot { board: BoardId, file: FileRef, frontier: Vec<(DeviceId, Head)>, change: u64, expect: Expect },
    Page { board: BoardId, expect: Expect },
    Declare { board: BoardId, expect: Expect },
    Upload { board: BoardId, sha256: [u8; 32], reference: Map<String, Value>, expect: Expect },
    Bind { board: BoardId, expect: Expect },
}

/// The boards of one App.
#[derive(Default)]
pub struct Scribble {
    pads: BTreeMap<BoardId, Pad>,
    /// The board in view.
    focus: Option<BoardId>,
    asked: BTreeMap<IoId, Asked>,
    /// Something shown changed since the views were made.
    dirty: bool,
    /// What went wrong, for the App's log.
    pub notes: Vec<String>,
}

/// Sends a request built for a route through the engine (its token, its retries); the id and what the route
/// expects, or none when the engine would not send it.
fn ask(engine: &mut Engine, ctx: &mut Ctx<'_>, build: impl FnOnce(&Site<'_>) -> Result<(HttpRequest, Expect), Failure>) -> Option<(IoId, Expect)> {
    let cell: Rc<RefCell<Option<Expect>>> = Rc::new(RefCell::new(None));
    let put = cell.clone();
    let id = engine
        .request(
            ctx,
            move |site| {
                let (request, expect) = build(site)?;
                *put.borrow_mut() = Some(expect);
                Ok(request)
            },
            None,
        )
        .ok()?;
    let expect = cell.borrow_mut().take()?;
    Some((id, expect))
}

/// The register's frontier (model form: `{writer hex: [seq, hash hex]}`), ascending by writer.
fn frontier_of(value: &Value) -> Option<Vec<(DeviceId, Head)>> {
    let mut out = Vec::new();
    for (writer, head) in value.as_object()? {
        let writer = DeviceId::from_slice(&unhex(writer)?).ok()?;
        let seq = head.get(0)?.as_u64()?;
        let hash = Hash32::from_slice(&unhex(head.get(1)?.as_str()?)?).ok()?;
        out.push((writer, Head { seq, hash }));
    }
    out.sort_by_key(|(w, _)| *w);
    Some(out)
}

fn frontier_value(frontier: &[(DeviceId, Head)]) -> Value {
    Value::Object(frontier.iter().map(|(w, h)| (hex(w.as_bytes()), json!([h.seq, hex(h.hash.as_bytes())]))).collect())
}

impl Scribble {
    /// The board in view now (none: no board shown). Read once the room is live.
    pub fn focus(&mut self, board: Option<BoardId>) {
        if let Some(id) = board {
            self.pads.entry(id).or_insert_with(|| Pad::new(id));
        }
        if self.focus != board {
            self.focus = board;
            self.dirty = true;
        }
    }

    /// A board to read though it is not in view (a desk's delete).
    pub fn read(&mut self, board: BoardId) {
        self.pads.entry(board).or_insert_with(|| Pad::new(board));
    }

    /// Whether something shown changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Whether an answered request is one of the boards'.
    pub fn owns(&self, id: IoId) -> bool {
        self.asked.contains_key(&id)
    }

    /// An envelope the engine reported: a board item of a board that is read is applied (or held while it is
    /// read).
    pub fn envelope(&mut self, r: &ReceivedEnvelope, now: u64) {
        let Subject::Item(Timeline::Board(board)) = r.header.subject else { return };
        let Some(pad) = self.pads.get_mut(&board) else { return };
        if matches!(r.outcome, EnvelopeOutcome::Refused | EnvelopeOutcome::Void) || pad.step == Step::Waiting {
            return;
        }
        let got = match (&r.body, &r.code) {
            (Some(body), _) => Got::Body(SecretBytes::new(body.payload().to_vec())),
            (None, Some(Error::NoKey)) => Got::Unread(Unread::NoKey),
            _ => return,
        };
        let held = Held { sender: r.header.sender, seq: r.header.seq, hash: r.envelope_hash, got };
        pad.newest_change = pad.newest_change.max(r.change);
        if pad.step == Step::Ready {
            pad.apply(held, now);
            self.dirty |= self.focus == Some(board);
        } else {
            pad.held.insert(r.change, held);
        }
    }

    /// A piece of a stroke someone is drawing (7.2).
    pub fn piece(&mut self, from: &DeviceId, board: &BoardId, piece: &[u8], now: u64) {
        let Some(pad) = self.pads.get_mut(board) else { return };
        let Some(p) = crate::codec::decode_piece(piece) else { return };
        let Some(ink) = p.get("points").and_then(Value::as_str).and_then(|t| Ink::from_base64url(t).ok()) else { return };
        let by = hex(from.as_bytes());
        let id = format!("live:{by}/{}", p.get("stroke").and_then(Value::as_str).unwrap_or(""));
        let (shape, at) = pad.live.entry(id.clone()).or_insert_with(|| {
            let shape = BoardShape {
                id,
                by,
                tool: p.get("tool").and_then(Value::as_str).unwrap_or("pen").into(),
                color: p.get("color").and_then(Value::as_str).map(String::from),
                width: p.get("width").and_then(Value::as_f64),
                live: true,
                ..Default::default()
            };
            (shape, now)
        });
        let tilted_before = shape.az.is_some() || shape.pts.is_empty();
        put_ink(shape, &ink);
        if !(tilted_before && ink.tilted()) {
            shape.az = None;
            shape.al = None;
        }
        *at = now;
        self.dirty |= self.focus == Some(*board);
    }

    /// The hub answered a request of the boards (or the engine gave up on it).
    pub fn answered(&mut self, id: IoId, result: &Result<crate::app::io::HttpResponse, Failure>, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        let answer = |expect: &Expect| match result {
            Ok(response) => Codec.answer(expect, response),
            Err(failure) => Err(failure.clone()),
        };
        match asked {
            Asked::Snapshot { board, file, frontier, change, expect } => {
                let loaded = match answer(&expect) {
                    Ok(Answer::File(got)) => trommi_core::files::decrypt_file(&file, &got.bytes)
                        .map_err(|e| e.code().to_string())
                        .and_then(|plain| gzip::gunzip(&plain, MAX_SNAPSHOT_LEN).map_err(|e| format!("{e:?}")))
                        .and_then(|json| Board::from_snapshot(&json, &frontier).map_err(|e| e.code().to_string())),
                    other => Err(format!("{:?}", other.err())),
                };
                let Some(pad) = self.pads.get_mut(&board) else { return };
                let after = match loaded {
                    Ok(b) => {
                        pad.snapshot_empty = b.shapes().next().is_none();
                        pad.reset(b, &frontier);
                        pad.newest_change = pad.newest_change.max(change);
                        change.saturating_sub(ITEMS_BEFORE_SNAPSHOT)
                    }
                    Err(why) => {
                        self.notes.push(format!("scribble: the snapshot does not load ({why}): the board is read from its start"));
                        pad.reset(Board::new(), &[]);
                        0
                    }
                };
                pad.step = Step::Tail;
                self.dirty = true;
                self.page(board, after, engine, ctx);
            }
            Asked::Page { board, expect } => {
                let Some(pad) = self.pads.get_mut(&board) else { return };
                pad.paging = false;
                match answer(&expect) {
                    Ok(Answer::Envelopes { items, more }) => {
                        let last = items.last().map(|i| i.change);
                        engine.show_items(ctx, &items);
                        match (more, last) {
                            (true, Some(last)) => self.page(board, last, engine, ctx),
                            _ => pad_done(self.pads.get_mut(&board)),
                        }
                    }
                    other => {
                        self.notes.push(format!("scribble: a page of the board was not read ({:?})", other.err()));
                        pad_done(Some(pad));
                    }
                }
            }
            Asked::Declare { board, expect } => {
                let ok = matches!(answer(&expect), Ok(Answer::Pruned { .. }));
                if !ok || !self.upload(board, engine, ctx) {
                    self.give_up(board, ctx.now);
                }
            }
            Asked::Upload { board, sha256, reference, expect } => {
                let ok = matches!(answer(&expect), Ok(Answer::FileStored { sha256: got, .. }) if got == sha256);
                if !ok || !self.seal_register(board, reference, engine, ctx) {
                    self.give_up(board, ctx.now);
                }
            }
            Asked::Bind { board, expect } => {
                // (not bound: the declaration keeps holding pruning back, and the next snapshot binds again)
                if let Err(why) = answer(&expect) {
                    self.notes.push(format!("scribble: the snapshot's frontier was not bound ({why:?})"));
                }
                if let Some(pad) = self.pads.get_mut(&board) {
                    pad.snapping = None;
                }
            }
        }
    }

    /// The hub took an outbox entry: a snapshot's register binds its frontier now.
    pub fn accepted(&mut self, outbox_id: u64, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some((board, pad)) = self.pads.iter_mut().find(|(_, p)| p.snapping.as_ref().and_then(|s| s.register).is_some_and(|r| r.0 == outbox_id)) else { return };
        let board = *board;
        let Some(snap) = pad.snapping.as_ref() else { return };
        let Some((_, seq, hash)) = snap.register else { return };
        pad.applied = pad.applied.saturating_sub(snap.covers);
        pad.snapshot_empty = snap.empty && pad.applied == 0;
        let frontier: Vec<(DeviceId, u64, Hash32)> = snap.frontier.iter().map(|(w, h)| (*w, h.seq, h.hash)).collect();
        let files = snap.files.clone();
        let asked = ask(engine, ctx, move |site| {
            Codec.request(site, &Route::BoardFrontier { board: &board, frontier: &frontier, files: &files, snapshot: Some((seq, hash)) })
        });
        match asked {
            Some((id, expect)) => {
                self.asked.insert(id, Asked::Bind { board, expect });
            }
            None => pad.snapping = None,
        }
    }

    /// The hub refused an outbox entry: a snapshot's register ends that snapshot.
    pub fn refused(&mut self, outbox_id: u64) {
        for pad in self.pads.values_mut() {
            if pad.snapping.as_ref().and_then(|s| s.register).is_some_and(|r| r.0 == outbox_id) {
                pad.snapping = None;
            }
        }
    }

    /// Writes a snapshot of `board` as soon as nothing of it is in flight, however few items it has: deleting a
    /// desk writes the snapshot of its empty board (`desk_delete::Step::WriteSnapshot`).
    pub fn snapshot_now(&mut self, board: BoardId) {
        self.pads.entry(board).or_insert_with(|| Pad::new(board)).force = true;
    }

    /// What this device knows of a board, for a desk's delete; none until it was read.
    pub fn known(&self, board: &BoardId) -> Option<BoardKnown> {
        let pad = self.pads.get(board).filter(|p| p.step == Step::Ready)?;
        Some(BoardKnown {
            shapes: pad.shapes.keys().map(shape_key).collect(),
            files: pad.refs.keys().cloned().collect(),
            notes: pad.shapes.values().filter(|s| matches!(s.tool.as_str(), "text" | "sticky" | "voice")).count() as u64,
            snapshot_empty: pad.snapshot_empty,
        })
    }

    /// The shapes on a board that is read, by their model ids (an erase of them all: a desk's delete).
    pub fn shape_ids(&self, board: &BoardId) -> Vec<String> {
        self.pads.get(board).map(|p| p.shapes.keys().map(shape_key).collect()).unwrap_or_default()
    }

    /// Reads, finishes and writes what is due: called after every event. `live`: the room is caught up and
    /// connected.
    pub fn tend(&mut self, now: u64, live: bool, model: &Model, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        // a board is read once the room is live (its snapshot register is current then)
        if live {
            let waiting: Vec<BoardId> = self.pads.iter().filter(|(_, p)| p.step == Step::Waiting).map(|(id, _)| *id).collect();
            for board in waiting {
                self.start(board, model, engine, ctx);
            }
        }
        for (board, pad) in &mut self.pads {
            if pad.step == Step::Tail && pad.tail_done && !pad.paging {
                for (_, item) in std::mem::take(&mut pad.held) {
                    pad.apply(item, now);
                }
                pad.step = Step::Ready;
                self.dirty |= self.focus == Some(*board);
            }
            let before = pad.live.len();
            pad.live.retain(|_, (_, at)| now < *at + LIVE_MS);
            self.dirty |= before != pad.live.len() && self.focus == Some(*board);
        }
        if !live {
            return;
        }
        let due: Vec<BoardId> = self
            .pads
            .iter()
            .filter(|(id, pad)| {
                pad.step == Step::Ready
                    && pad.snapping.is_none()
                    && pad.board.unread().is_none()
                    && (pad.force || (pad.applied >= SNAP_EVERY && now >= pad.last_at + SNAP_IDLE_MS))
                    && !own_in_flight(model, id)
            })
            .map(|(id, _)| *id)
            .collect();
        for board in due {
            self.declare(board, now, engine, ctx);
        }
    }

    /// When the boards want to be looked at again by the clock.
    pub fn wake(&self, now: u64) -> Option<u64> {
        self.pads
            .values()
            .flat_map(|pad| {
                let snap = (pad.step == Step::Ready && pad.snapping.is_none() && pad.applied >= SNAP_EVERY)
                    .then_some(pad.last_at + SNAP_IDLE_MS)
                    .filter(|at| *at > now);
                let live = pad.live.values().map(|(_, at)| at + LIVE_MS).min();
                [snap, live]
            })
            .flatten()
            .min()
    }

    /// The board in view as the shell draws it: the board's shapes, this device's items that are not back from
    /// the hub laid over them, and the strokes others are drawing.
    pub fn view(&self, model: &Model) -> Option<BoardView> {
        let board = self.focus?;
        let pad = self.pads.get(&board)?;
        let mut shapes: Vec<BoardShape> = pad.shapes.values().cloned().collect();
        overlay(&mut shapes, pad, model);
        // by layer, then as made (the sort keeps the order of equals)
        shapes.sort_by_key(|s| s.z);
        shapes.extend(pad.live.values().map(|(s, _)| s.clone()));
        Some(BoardView { board_id: format!("desk/{}", pad.hex()), state: pad.state().into(), shapes })
    }

    /// The references of the pictures on the board in view (so that `OpenFile` opens them).
    pub fn pictures(&self, model: &Model) -> BTreeMap<String, Map<String, Value>> {
        let Some(pad) = self.focus.and_then(|b| self.pads.get(&b)) else { return BTreeMap::new() };
        let mut out = pad.refs.clone();
        for (_, _, content) in pending(model, pad) {
            for shape in content.get("strokes").and_then(Value::as_array).into_iter().flatten() {
                if let Some(r) = shape.get("attachment").and_then(Value::as_object) {
                    if let Some(id) = r.get("attachment_id").and_then(Value::as_str) {
                        out.insert(id.to_string(), r.clone());
                    }
                }
            }
        }
        out
    }

    // ---- reading

    fn start(&mut self, board: BoardId, model: &Model, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some(pad) = self.pads.get_mut(&board) else { return };
        let key = format!("scribble_snapshot/desk/{}", pad.hex());
        let named = model.human.get(&key).map(|r| &r.value).filter(|v| !v.is_null()).and_then(|v| {
            let file = file_ref(v.get("attachment")?.as_object()?)?;
            let frontier = frontier_of(v.get("frontier")?)?;
            let change = v.get("last_envelope_number").and_then(Value::as_u64).unwrap_or(0);
            Some((file, frontier, change))
        });
        pad.tail_done = false;
        pad.held.clear();
        self.dirty = true;
        if let Some((file, frontier, change)) = named {
            let file_id = file.file_id;
            if let Some((id, expect)) = ask(engine, ctx, move |site| Codec.request(site, &Route::GetFile { file: &file_id, range: None })) {
                pad.step = Step::Snapshot;
                self.asked.insert(id, Asked::Snapshot { board, file, frontier, change, expect });
                return;
            }
        }
        pad.reset(Board::new(), &[]);
        pad.step = Step::Tail;
        self.page(board, 0, engine, ctx);
    }

    fn page(&mut self, board: BoardId, after: u64, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        match ask(engine, ctx, move |site| Codec.request(site, &Route::Board { board: &board, after, limit: PAGE })) {
            Some((id, expect)) => {
                if let Some(pad) = self.pads.get_mut(&board) {
                    pad.paging = true;
                }
                self.asked.insert(id, Asked::Page { board, expect });
            }
            None => pad_done(self.pads.get_mut(&board)),
        }
    }

    // ---- writing a snapshot

    fn give_up(&mut self, board: BoardId, now: u64) {
        if let Some(pad) = self.pads.get_mut(&board) {
            pad.snapping = None;
            // (tried again after the next quiet while)
            pad.last_at = now;
        }
    }

    /// Step 1: the frontier is declared, so that the hub holds pruning back for it.
    fn declare(&mut self, board: BoardId, now: u64, engine: &mut Engine, ctx: &mut Ctx<'_>) {
        let Some(pad) = self.pads.get_mut(&board) else { return };
        pad.force = false;
        let frontier: Vec<(DeviceId, Head)> = pad.heads.iter().map(|(w, h)| (*w, *h)).collect();
        let files: Vec<FileId> = pad.refs.keys().filter_map(|a| FileId::from_slice(&unhex(a)?).ok()).collect();
        let snapping = Snapping {
            frontier: frontier.clone(),
            files: files.clone(),
            covers: pad.applied,
            newest_change: pad.newest_change,
            empty: pad.shapes.is_empty(),
            register: None,
        };
        let wire: Vec<(DeviceId, u64, Hash32)> = frontier.iter().map(|(w, h)| (*w, h.seq, h.hash)).collect();
        match ask(engine, ctx, move |site| Codec.request(site, &Route::BoardFrontier { board: &board, frontier: &wire, files: &files, snapshot: None })) {
            Some((id, expect)) => {
                pad.snapping = Some(snapping);
                self.asked.insert(id, Asked::Declare { board, expect });
            }
            None => self.give_up(board, now),
        }
    }

    /// Step 2: the file, packed and encrypted, to the hub.
    fn upload(&mut self, board: BoardId, engine: &mut Engine, ctx: &mut Ctx<'_>) -> bool {
        let Some(pad) = self.pads.get_mut(&board) else { return false };
        let Some(snap) = pad.snapping.as_ref() else { return false };
        let json = match pad.board.snapshot(&snap.frontier) {
            Ok(json) => json,
            Err(e) => {
                self.notes.push(format!("scribble: no snapshot of this board ({})", e.code()));
                return false;
            }
        };
        let packed = gzip::gzip(json.expose());
        let Ok((stored, sealed)) = engine.with_entropy(|entropy| trommi_core::files::encrypt_file(&packed, entropy)) else { return false };
        let reference = reference_of(&sealed.file, SNAPSHOT_FILE, SNAPSHOT_TYPE, sealed.plain_len, None, None);
        let sha256 = *sealed.file.sha256.as_bytes();
        let file = sealed.file.file_id;
        match ask(engine, ctx, move |site| Codec.request(site, &Route::PutFile { file: &file, bytes: &stored })) {
            Some((id, expect)) => {
                self.asked.insert(id, Asked::Upload { board, sha256, reference, expect });
                true
            }
            None => false,
        }
    }

    /// Step 3: the register names the new snapshot.
    fn seal_register(&mut self, board: BoardId, reference: Map<String, Value>, engine: &mut Engine, ctx: &mut Ctx<'_>) -> bool {
        let Some(pad) = self.pads.get_mut(&board) else { return false };
        let Some(snap) = pad.snapping.as_mut() else { return false };
        let key = format!("scribble_snapshot/desk/{}", hex(board.as_bytes()));
        let file = reference.get("attachment_id").and_then(Value::as_str).and_then(unhex).and_then(|b| FileId::from_slice(&b).ok());
        let value = json!({ "attachment": reference, "frontier": frontier_value(&snap.frontier), "last_envelope_number": snap.newest_change });
        let Ok((name, Some(wire))) = crate::codec::encode_register(&key, &value) else { return false };
        let draft = Draft::Register { group: GroupId::room(engine.room()), name, value: Some(SecretBytes::new(wire.into_bytes())) };
        let files: Vec<FileId> = file.into_iter().collect();
        match engine.act(ctx, |device, now| device.seal(&draft, None, &files, now)) {
            Ok(sealed) => {
                snap.register = Some((sealed.outbox_id, sealed.seq, sealed.envelope_hash));
                true
            }
            Err(e) => {
                self.notes.push(format!("scribble: the snapshot's register was not sealed ({})", e.code()));
                false
            }
        }
    }
}

fn pad_done(pad: Option<&mut Pad>) {
    if let Some(pad) = pad {
        pad.paging = false;
        pad.tail_done = true;
    }
}

/// This device's items of a board that are not back from the hub: their number in its chain and their content
/// (the model's form), in the order they were sealed.
fn pending<'m>(model: &'m Model, pad: &Pad) -> Vec<(u64, Option<&'m str>, &'m Map<String, Value>)> {
    let key = crate::model::timeline_key("scribble", &format!("desk/{}", pad.hex()));
    let Some(timeline) = model.timelines.get(&key) else { return Vec::new() };
    let me = model.my_device_id.as_deref().and_then(unhex).and_then(|b| DeviceId::from_slice(&b).ok());
    let applied = me.map_or(0, |me| pad.board.applied(&me));
    let mut out: Vec<_> = timeline
        .items
        .values()
        .filter(|item| item.get("pending") == Some(&Value::Bool(true)))
        .filter_map(|item| {
            let seq = item.get("sender_sequence")?.as_u64()?;
            Some((seq, item.get("local_id").and_then(Value::as_str), item.get("content")?.as_object()?))
        })
        .filter(|(seq, _, _)| *seq > applied)
        .collect();
    out.sort_by_key(|(seq, _, _)| *seq);
    out
}

/// Whether an own item of the board is still on its way to the hub.
fn own_in_flight(model: &Model, board: &BoardId) -> bool {
    let key = crate::model::timeline_key("scribble", &format!("desk/{}", hex(board.as_bytes())));
    model.timelines.get(&key).is_some_and(|t| t.items.values().any(|i| i.get("pending") == Some(&Value::Bool(true))))
}

/// Lays this device's pending items over the board's shapes, as the board will take them.
fn overlay(shapes: &mut Vec<BoardShape>, pad: &Pad, model: &Model) {
    let me = model.my_device_id.clone().unwrap_or_default();
    let Some(sender) = unhex(&me).and_then(|b| DeviceId::from_slice(&b).ok()) else { return };
    for (seq, local, content) in pending(model, pad) {
        let Ok(payload) = crate::codec::encode_body(crate::codec::BodyKind::BoardItem, content) else { continue };
        let Ok(body) = ItemBody::decode(payload.as_bytes()) else { continue };
        match body {
            ItemBody::Strokes(list) => {
                for (index, shape) in (0u32..).zip(&list) {
                    let (mut view, _) = view_of(shape_key(&ShapeId { sender, seq, index }), me.clone(), shape);
                    view.local = local.map(|l| format!("{l}/{index}"));
                    shapes.push(view);
                }
            }
            ItemBody::Erase(ids) | ItemBody::SendAway(ids) => {
                let gone: BTreeSet<String> = ids.iter().map(shape_key).collect();
                shapes.retain(|s| !gone.contains(&s.id));
            }
            ItemBody::Move { shapes: ids, offset } => {
                let moved: BTreeSet<String> = ids.iter().map(shape_key).collect();
                let (dx, dy) = (units(offset[0]), units(offset[1]));
                for s in shapes.iter_mut().filter(|s| moved.contains(&s.id)) {
                    for (i, v) in s.pts.iter_mut().enumerate() {
                        *v += if i % 2 == 0 { dx } else { dy };
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
