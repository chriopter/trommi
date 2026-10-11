# Changelog

What changed in Trommi, newest first. One section per day; the release numbers it covers are in brackets.

## 11 October 2026 (v109 – v115)

**New**
- Log out right from the desk menu.
- The connector updates itself while it runs: no more reconnect after an update.
- Drag sessions in the sidebar to put them in your order (web; the iPhone shows the same order).

**Better**
- Settings: Invite a Device and Invite an Agent stand side by side as equals; Devices and Account are laid out like the rest of Settings, each device and each way in (password, passkeys, Emergency Kit) with a page of its own.
- An agent that reports an ongoing matter keeps one card and updates it, newest state in the title.
- The self-test screen is now called "Security Info".

**Fixed**
- A device that missed the keys for older content now asks your other devices for them, even weeks later.
- Changing your password, Emergency Kit or passkeys now asks for a current one: being logged in is not enough.
- A phone added to a running session could show the agent's raw id instead of its name.
- The iPhone app could crash while sending (a data race).
- After logging out, the browser kept the key that had protected its local data.

## 10 October 2026 (v93 – v108)

**New**
- History graphs on the hub's admin page.
- "New Agent…" makes a fresh link each time; the previous unused link stops working.

**Fixed**
- A corner note with only pictures could not be sent.
- Deleting a session failed with "ids is not defined".
