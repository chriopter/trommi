#!/usr/bin/env node
// mega-fixture.mjs: a big made-up room for speed tests, grown from the demo room (demo/data/fixture.json).
// The demo is copied COPIES times with fresh ids (agents, sessions, cards, permissions, artifacts) and envelope
// numbers moved by a million per copy; the human devices, the desks and the corner note stay one. The crowned
// session of the first copy gets a chat of CHAT items (his words, the agent's answers, work trails, details).
// All of it is made up, as the demo is.
//
//   node tests/tools/mega-fixture.mjs [out.json] [COPIES=10] [CHAT=5000]
//
// Read by tests/ios/TrommiClientTests/SpeedTests.swift (TROMMI_SPEED=1) and, on the simulator, by the app's demo
// (TROMMI_FIXTURE=<path>, DemoMode.swift).
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const out = process.argv[2] ?? join(tmpdir(), "trommi-mega.json");
const COPIES = Number(process.argv[3] ?? 10);
const CHAT = Number(process.argv[4] ?? 5000);
const base = JSON.parse(readFileSync(join(here, "../../demo/data/fixture.json"), "utf8"));

const humans = new Set(base.members.filter((m) => m.device_role === "human").map((m) => m.device_id));
// the ids a copy renames: agent devices, sessions, cards, permissions, artifacts
const ids = new Set();
for (const m of base.members) if (!humans.has(m.device_id)) ids.add(m.device_id);
for (const s of base.sessions) { ids.add(s.agent_device_id); if (s.session_id) ids.add(s.session_id); }
for (const c of base.cards) ids.add(c.object_id);
for (const p of base.permissions) ids.add(p.object_id);
for (const p of base.published) ids.add(p.object_id);
const fresh = (id, k) => createHash("sha256").update(`${id}/${k}`).digest("hex").slice(0, id.length);
const NUMBER_KEYS = /(^|_)envelope_number$|^heard_up_to$/;

function copyOf(v, k) {
  if (k === 0) return v;
  let text = JSON.stringify(v);
  for (const id of ids) text = text.split(id).join(fresh(id, k));
  // (numbers moved by a million per copy; a session's name for its agent is its id on the Desk: made unique too)
  return JSON.parse(text, (key, x) => (typeof x === "number" && NUMBER_KEYS.test(key) ? x + k * 1_000_000
    : key === "agent_session_id" && typeof x === "string" ? `${x}-${k}` : x));
}

const room = structuredClone(base);
room.members = base.members.filter((m) => humans.has(m.device_id));
room.sessions = []; room.cards = []; room.permissions = []; room.published = [];
room.timelines = {};
for (const [key, items] of Object.entries(base.timelines)) if (key.startsWith("scribble:")) room.timelines[key] = items;
room.human.session_settings = {}; room.human.snoozes = {}; room.human.ducks = {}; room.human.drafts = {};
for (let k = 0; k < COPIES; k++) {
  const c = copyOf({
    members: base.members.filter((m) => !humans.has(m.device_id)), sessions: base.sessions, cards: base.cards, permissions: base.permissions,
    published: base.published, timelines: Object.fromEntries(Object.entries(base.timelines).filter(([key]) => key.startsWith("chat:"))),
    session_settings: base.human.session_settings, snoozes: base.human.snoozes, ducks: base.human.ducks, drafts: base.human.drafts,
  }, k);
  room.members.push(...c.members); room.sessions.push(...c.sessions); room.cards.push(...c.cards);
  room.permissions.push(...c.permissions); room.published.push(...c.published);
  Object.assign(room.timelines, c.timelines);
  Object.assign(room.human.session_settings, c.session_settings); Object.assign(room.human.snoozes, c.snoozes);
  Object.assign(room.human.ducks, c.ducks); Object.assign(room.human.drafts, c.drafts);
}

// the long chat: the crowned session of the first copy
const agent = base.human.desks.main.crown.agent_device_id;
const me = base.room.my_device_id;
const key = `chat:session/${agent}`;
const chat = room.timelines[key] ?? (room.timelines[key] = []);
const start = (COPIES + 1) * 1_000_000;
const t0 = base.made_at - CHAT * 20_000;
const words = ["the build", "the landing page", "push reminders", "the checkout", "the design tokens", "the onboarding", "the iPad layout", "the release notes"];
for (let i = 0; i < CHAT; i++) {
  const n = start + i * 10, mine = i % 7 === 0, trail = !mine && i % 5 === 2;
  const w = words[i % words.length];
  const content = mine ? { text: `Please look at ${w} again (${i}).`, attachments: [] }
    : trail ? { text: `Read ${w}: 3 files, ran the tests.`, attachments: [], terminal: "work" }
    : { text: `Done with ${w}: ${i % 3 === 0 ? "**two** fixes, one test added, see the details." : "all green."}`, attachments: [],
        ...(i % 11 === 0 ? { details: `| step | ms |\n| --- | --- |\n| build | ${100 + i % 50} |\n| test | ${200 + i % 70} |` } : {}) };
  chat.push({ envelope_number: n, local_id: null, pending: false, envelope_hash: createHash("sha256").update(`m${i}`).digest("hex"),
    sender_device_id: mine ? me : agent, recipient_device_id: mine ? agent : null, sent_at: t0 + i * 20_000, item_state: "loaded",
    content_type: "message", content });
}
room.room.last_envelope_number = start + CHAT * 10;
writeFileSync(out, JSON.stringify(room));
const items = Object.values(room.timelines).reduce((a, t) => a + t.length, 0);
console.log(`${out}: ${room.sessions.length} sessions, ${room.cards.length} cards, ${room.permissions.length} permissions, ${Object.keys(room.timelines).length} timelines, ${items} chat items`);
