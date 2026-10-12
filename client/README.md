# trommi-client-core

The client core: what every Trommi client decides the same way, written once in Rust. It sits above the protocol
core ([`core/`](../core/README.md): MLS, envelopes, chains, the command gate) and below the apps. It does no IO: the
host fetches, stores and draws; the client core decides.

```
   app/web (TypeScript)        app/ios (Swift)          a Rust host
   network, stream,            network, stream,         (a Linux client,
   IndexedDB, worker, UI       files, Keychain, SwiftUI  planned)
          │                           │                        │
          │ wasm (core/wasm)          │ UniFFI (core/swift)    │ the crate itself
          └────────────┬──────────────┘                        │
                       ▼                                       ▼
          ClientDesk in the facade (core/swift/src/client.rs) ─┘
                       │
                       ▼
                 client/  (this crate)
          the Desk rules, the load plan, the board facts
                       │
                       ▼
                 core/  (trommi-core)
          MLS, envelopes, chains, gate, files
```

## What lives here

| File | What | Used by |
| --- | --- | --- |
| `src/desk.rs` | `DeskState`: the Desk of one device. The hub's open list decides (`Listing`): an object open here that the list leaves out, with nothing newer here than the list's change, is held (settled elsewhere); while the app waits for the hub's first list everything open is held (`set_hold_all`). The stack order (urgency, time asked, agent, id), snoozes and archived sessions, each session's open cards, the open permission requests, a request whose time ran out (its deadline, or unread and older than `REQUEST_LIFE_MAX_MS`), a hand back older than the version the hub lists, and when to look again by the clock (`next_wake`). `project(now)` answers with a `Delta`: only what changed (stack and session edits, held and unheld), the whole Desk only the first time and after `clear`. | web, iOS (through `ClientDesk`) |
| `src/codec.rs` | The bodies of stored content (spec 9.1) between the wire and the model, both ways: ids (base64url ↔ hex), attachment references (`file_id` ↔ `attachment_id`), a message's `artifact_object_id` ↔ `published_object_id`, an Artifact's `shared_until` ↔ `released_until`, register names and the ids in their values, board items (`shape_ids`, 1/16 units ↔ `stroke_ids`, board units) and stroke pieces, read as trommi-core's `board_items` and `trail` read them. `decode_body` (the model's fields), `decode_body_text` (the same as JSON text, read from the body's own text without building it whole: the hot path), `encode_body`, `encode_register`, `register_from_wire`, `file_ids_of`, `decode_piece`, `encode_piece`. The numbers follow JavaScript's, which the web wrote first. | web (`codec.ts`, and every received envelope's `body`), the board model |
| `src/model/` | The board model (`Model`): cards, answers, requests, notes, published objects, timelines, registers, members and sessions as every app shows them, built from what the protocol core accepted; its Desk is `desk.rs`'s, and each open card carries its snooze as the Desk holds it (`snoozed_until`, `unsnoozed`). `finish(now)` hands an app the patch of what a batch touched, with `wake_at`: when to project again by the clock alone. | web (`ClientModel`, mirrored in `mirror.ts`), iOS (`BoardMirror.swift`) |
| `src/sessions.rs` | The session list (`SessionRow`): which sessions show, their names, online or gone, archived (the person's choice, a closed helper, a replaced pairing of the same folder), parent, desk and order. The Desk leaves out the cards of the sessions it lists as archived. | web (`BoardState.agents`), iOS (`DeskModel.agents`) |
| `src/status.rs` | Each session's link (`live`, `oncall`, `asleep`, `cut`, `gone`), its red hand (`blocked`: cut off, gone while working, or a permission request or a blocking question that stands on the Desk) and its quiet hint, decided at the model's clock, with the moment it may change (`recheck_at`). | web and iOS, through the session list |
| `src/presence.rs` | Presence from the live stream: a `presence` event (`apply_presence_event`: no `hears`, the link report stands; offline as of the event; `lost` an alert) and the opening list of who is here, whole once its `here` came (`stream_opened`, `apply_here`: a device shown online that the list did not name is offline). | web (`client.ts onStream`), iOS (`Room` stream) |
| `examples/codec_speed.rs` | The codec alone: 10 000 card and message bodies read. | by hand |
| `src/plan.rs` | The load rules of a catch-up as steps (`CatchUp`, `Step`): the Desk once (then its changes since), the open objects' current versions, the history once, Welcomes once per page, then the live stream. | a Rust host, tests |
| `src/board.rs` | Board facts (`Fact`, `Source`, `Board`): newest to oldest, what history brings may only close. | a Rust host, tests |
| `src/registers.rs` | Session names and settings as registers; a register never makes a session. | a Rust host, tests |
| `src/store.rs` | The store interface a Rust host gives (`Store`, `MemoryStore`). | a Rust host |
| `examples/desk_speed.rs` | The Desk alone: one card at a time and pages of 200. | by hand |

## What stays in each app

- **Network and the live stream:** the hub's routes, retries, the event stream (web `hub.ts`, iOS `Hub.swift`,
  `SSE.swift`; the connector `hub.rs`).
- **Storage:** IndexedDB and the tabs (web), files and the Keychain (iOS), the journal (connector).
- **The sync engine:** the hub's order, Welcomes, a group's past, gaps in a sender's chain (web `engine.ts`,
  `room.ts`; iOS `Room*.swift`; connector `client.rs`).
- **A copy of the board model** to render from, kept by the core's patches (web `mirror.ts`, iOS `BoardMirror.swift`);
  the connector keeps its own (`model.rs`).
- **Words:** what a link, a red hand or a snooze says; the core decides which, the app says it.
- **The UI:** the web's views, SwiftUI; the connector's MCP tools.

## The body codec across the facade

`core/swift/src/codec.rs` hands the codec to both bindings as JSON text: `body_from_wire`, `bodies_from_wire`,
`body_to_wire`, `board_item_parse`, `register_to_wire`, `register_from_wire`, `register_key_of`,
`register_name_of`, `body_file_ids`, `attachment_from_wire`, `attachment_to_wire`, `board_entry_from_wire`,
`shape_id_from_wire`, `shape_id_to_wire`, `piece_from_wire`, `piece_to_wire`. A write the codec refuses is
`bad-format` with its own code at the head of the message (`bad-argument: …`). Reading is on every envelope that
arrives, so it is done where the payload already is: `ReceivedEnvelope.body` carries the body read as the kind its
header names (the model's fields as JSON text, or `newer_schema`, or `bad`); a call per body from JavaScript costs
about 2 µs in the binding's layer alone.

## How an app uses it

The app keeps one `ClientDesk` per room. After each batch it hands over only what the batch touched (`put_cards`,
`put_requests`, `remove`, `set_snooze`, `set_archived`), and the hub's list when it read one (`set_listing`), then
calls `project(now)` and applies the `Delta` to its own lists. The web app hands a batch as one JSON text
(`put_cards_json`, `project_json`): one crossing per batch instead of one record per card. iOS uses the records.

## Tests

`cargo test -p trommi-client-core`. `deltas_give_the_whole_desk` checks, over long seeded random runs, that the
deltas applied in order always give the Desk projected from nothing. Across the apps:
`tests/client/mega.rs` (see the root README, "Build and tests").
