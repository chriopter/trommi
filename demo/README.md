# The demo room

The room every Trommi client shows as its demo (`?mock=1` on the web), and the list of screens and states the web
app's review page `/screens` walks through. **All of it is made up**: no real room, account, person or address is in
here, and none may be added.

The room is a made-up plant-care company, Fernly, at the end of a launch week. It has two desks, "main" (Fernly: the
website, the iOS app, the design system) and "platform" (Platform: payments, the data pipeline, infrastructure, the
docs), each with its goals. On them work seven main sessions (two of them Codex) with 31 helpers, 2 to 8 each, all
with plain names ("Fernly Website", "Push Reminders"); each session has its conversation, some with work trails, notes
and artifacts in it. There are 52 cards: 31 open (yes/no, two and three options with pictures, multiple choice,
sections, a table, every urgency, two handed back to their agent), and answered, closed, settled, read, shredded,
withdrawn and merged ones; 14 artifacts, two permission requests already answered, the corner note, a Scribble Board
per desk and one for All desks, and the 40 pictures, pages and files they name. The Desk is meant to look inviting:
one blocking card, one that knocks, then design and product choices with pictures (oldest first), the plain
questions after them and the reports last. `/screens` lists the states of the account screens, the Desk on each desk, chats, cards,
Settings and the Scribble Board.

```
demo/
├── README.md
├── check.mjs          the check, and the one written form of the two JSON files
└── data/              what a client ships: only data
    ├── fixture.json   the room
    ├── screens.json   every screen and state
    └── files/         the pictures and pages the room names (absent while it names none)
```

## The data

**`data/fixture.json`**: one room read by the shared app core (`core/client/src/views/demo.rs`), as JSON:

| Key | What |
| --- | --- |
| `made_at` | the moment the room was made (ms). Every other time in the file is relative to it: a client adds `now - made_at` to each when it loads the room, so "5 min ago" stays five minutes ago |
| `room` | `room_id`, `hub_url` (`mock:`), `my_device_id`, `my_role`, the counters |
| `members` | the devices: at least the human one that looks at the room (`room.my_device_id`) |
| `sessions` | the agents' sessions with profile, status lines and settings |
| `cards` | the decision and info cards, each with its versions and answers |
| `permissions`, `notes`, `published` | waiting permission requests, the corner note, published artifacts |
| `timelines` | the conversations (`chat:session/<device>`, `chat:card/<object>`) and the Scribble Boards (`scribble:desk/<id>`) |
| `human` | the human registers: `drafts`, `snoozes`, `ducks`, `crown`, `desks`, `session_settings` |

An attachment names its file as `url: "/demo/files/<name>"`; the file is `data/files/<name>`.

**`data/screens.json`**: `{ "states": [...] }`, one line per state, a screen's states together and its first one the
screen as it is:

```json
{"id":"desk--keys-sheet","screen":"Desk","state":"Keys sheet","web":{"path":"/","state":"keys","mock":"1"}}
```

| Field | What |
| --- | --- |
| `id` | `<screen>` for the first state of a screen, else `<screen>--<state>`, both in lower case with `-` for everything else |
| `screen`, `state` | the names shown |
| `web.path` | the address in the app |
| `web.state` | the click the web makes once the page is there (`?state=<name>`, `demoState` in `app/web/public/demo/demo.mjs`), or `null` |
| `web.mock` | the demo marker: `"1"` selects the fixture with times moved to now; historical marker names do not create a second core model |

Card routes name a fixture `object_id`; chat routes name the core session id (the first 32 characters of its
`agent_device_id`). The same paths drive the web review page and native `TROMMI_SCREEN` launches. The check
rejects routes whose card or session is missing, so a stale alias cannot silently produce an empty screen.

An empty list is allowed: `/screens` then says that none are listed.

## Who takes it, and when

The web app's build (`app/web/dev/build.mjs`) runs the check, then copies the room (without its spaces) to
`public/demo/fixture.json` and the files to `public/demo/files/`, the addresses the app fetches, and puts the states
into the bundle. Neither is in git. The dev server (`app/web/dev/serve.mjs`) does the same from memory on every
request: an edit here shows at the next reload.

The native app bundles this same data through `app/ios/TrommiApp/Sources/TrommiApp/Resources/Demo`.
`NativeDemo.swift` reads files and translates the catalog route into a core Focus event; it holds no room model.
Native UI-specific states are not all mapped yet.

## The check

`node demo/check.mjs` (`npm run test:demo` in `app/web/`; also run by `npm run test:app` and by the web app's build). It fails when

- `fixture.json` or `screens.json` does not parse, lacks a key, has a list or an object of the wrong kind, names no
  human device as the one looking, or is not in its written form (`node demo/check.mjs --write` writes both);
- a state has no id, an id twice, an id that does not follow from its names, or stands apart from its screen;
- a card or chat route names no object or session in the fixture; the browser gate also requires real page content;
- the fixture, `demo.mjs` or a page in `files/` names a file that is not there, or a file is named by nobody;
- the web app keeps a copy: anything of the demo data tracked in `app/web/public/demo/`.
