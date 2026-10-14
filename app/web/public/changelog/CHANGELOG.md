# What's new: one summary a day

The summary of each day for What's new (web and iPhone), newest first: one to three short sentences that name the
day's real highlights, the way you would tell a friend. Nothing else goes here: the day's changes themselves come
from the GitHub releases of each part (web-v, hub-v, connector-v, ios-v, mac-v), whose notes are the merged pull
requests' "What's new" sections; app.trommi.com/whats-new.json puts both together. Pull requests add nothing to this
file.

Written or updated once a day by whoever ships last that day, in its own small pull request: read the day's release
notes, pick the two to four things people will notice most. A day may have at most one picture, a line
`![what it shows](<YYYY-MM-DD>.webp)` under its summary: a file in this folder, a screenshot of the demo room only
(`?mock=1`, never real data), webp, at most 150 KB. The days before the releases had notes are in history.json
(frozen).

## 14 October 2026

The Linux app runs only on the shared Rust app core, which now handles accounts, invites, joining by link, files,
devices, sign-out, account deletion and archiving. A new sign-in loads only the open cards and what is new: about
1.6 s instead of 8 s on a large account, with names shown at once. The connector no longer re-joins as a new agent
after a resume, update finds new releases again, and the board's session list stays stable across loads.

## 13 October 2026

Signing in takes under a second, and every helper keeps its name and cards afterwards. Running work lies as a stack
of live cards with chips under the chat, and every card keeps its whole history. A first Linux app, and each part of
Trommi now ships on its own.

## 12 October 2026

Star the cards you want to keep, and a new agent session joins with one tap. Web and iPhone now agree on snoozes,
drafts and settings.

![Starred cards on top of Off your mind, in the demo room](2026-10-12.webp)

## 11 October 2026

Agents get their own page. Answer from the Lock Screen. Pushes give nothing away.

![Settings → Agents in the demo room](2026-10-11.webp)

## 10 October 2026

One encryption core everywhere. The connector installs with one line. A new demo room.

![The Desk of the demo room](2026-10-10.webp)

## 9 October 2026

Goals for each desk. The connector on macOS. Live Activities on the iPhone.

![A desk's goals under the greeting, in the demo room](2026-10-09.webp)

## 8 October 2026

The iPhone app is here: Desk, chats, notes and the Scribble Board, with push.

## 7 October 2026

Calmer on a phone. Six emoji to connect an agent. Blitz for rapid answers.

## 6 October 2026

Hand a session on. Duck it answers everything at once. Later puts a card off.

## 5 October 2026

Permission prompts reach the board. Each desk shows only its own sessions.

## 4 October 2026

End-to-end encryption. Accounts, QR pairing, the Scribble Board and a new Desk.

## 3 October 2026

Push notifications: a new question reaches your phone at once.

## 2 October 2026

The first board: your agents ask, you answer with a tap.
