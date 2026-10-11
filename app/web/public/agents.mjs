// Settings · Agents (<base>/settings/agents): the concrete agents, each a program on a computer ("Claude Code on
// MacBook Pro", "Codex on build-01"), as the connector reports them in its device register (platform, host, folder;
// the session's model where an older connector named no platform), and under each, as the branches of a tree, its
// main sessions: name, "<desk> desk · folder", "+N helpers", connected or away; a tap opens the session. An agent's
// "…" removes it from the account (its agent devices, as Remove under Devices did). Renaming an agent and updating its
// connector from here have no way on the hub yet: the connector names itself and updates itself.
// The old Settings · Sessions (/settings/sessions) leads here.
// The routes of a session's forms (<base>/sessions/<id>/…) are here too: a session's own page and the sidebar use them.
import { PLUS, SET_CHEVRON, agentsOf, usageFull, avatar, agoSpan, html, marksFrame, sessionForms, settingsPage, sk, sayError } from './ui.mjs'

const dot = on => html`<i class="set-dot" data-online="${on ? 'yes' : 'no'}"></i>`
const agentName = g => html`${g.program} <span>on ${g.host || 'an unnamed computer'}</span>`
function agentCard(g, i, base, deskName) {
  const helpers = g.sessions.length - g.tops.length
  return html`<section class="set-group ag-card" id="agent-${i}" aria-label="${g.program} on ${g.host || 'an unnamed computer'}">
<div class="ag-head"><span class="ag-head-ico">${sk('terminal')}</span><span class="ag-head-words"><b>${agentName(g)}</b><small>${dot(g.online)}${g.online ? 'connected' : html`away · last seen ${agoSpan(g.seen)}`} · ${g.tops.length} ${g.tops.length === 1 ? 'session' : 'sessions'}${helpers ? `, ${helpers} ${helpers === 1 ? 'helper' : 'helpers'}` : ''}</small></span>
${g.devices.length ? html`<details class="ag-more"><summary title="More" aria-label="More for ${g.program} on ${g.host || 'an unnamed computer'}">${sk('more')}</summary><form method="post" action="${base}/agents/remove" class="ag-menu"><input type="hidden" name="key" value="${g.key}"><p>${g.program} on ${g.host || 'this computer'} and its ${g.sessions.length} ${g.sessions.length === 1 ? 'session' : 'sessions'} can read nothing new after this. The others get a new key; the history stays.</p><button type="submit" class="room-danger">Remove from This Account</button></form></details>` : ''}</div>
${usageFull(g.sessions.find(a => a.main && a.account)?.account ?? g.sessions.find(a => a.account)?.account)}<ul class="ag-tree">${g.tops.map(({ a, helpers: n, folder }) => html`<li><a class="ag-sess" href="${base}/chat/${encodeURIComponent(a.id)}" data-nav>${avatar(a, { crown: false })}<span class="set-two"><b>${a.name}</b><small>${deskName(a)} desk${folder ? html` · <span class="room-mono">${folder}</span>` : ''}</small></span>${n ? html`<span class="ag-helpers">+${n} ${n === 1 ? 'helper' : 'helpers'}</span>` : html`<span></span>`}<span class="set-detail">${dot(a.online)}${a.online ? 'connected' : 'away'}</span>${SET_CHEVRON}</a></li>`)}</ul></section>`
}
/** The page's main. groups: agentsOf(); deskName(a): the name of a session's desk. */
export function agentsMain(groups, base, deskName, { error = '' } = {}) {
  const n = groups.reduce((s, g) => s + g.sessions.length, 0), computers = new Set(groups.map(g => g.host)).size
  return settingsPage('Agents', html`${error ? html`<p class="room-error" role="alert">${error}</p>` : ''}<div id="agents-list">${groups.length ? groups.map((g, i) => agentCard(g, i, base, deskName)) : html`<div class="set-group"><p class="set-empty">No agent yet.</p></div>`}</div>
<div class="set-group"><form method="post" action="${base}/pair" class="set-agent"><input type="hidden" name="role" value="agent"><button type="submit" class="set-row" id="agents-invite"><span class="set-ico">${PLUS}</span><b>Invite an Agent…</b><span class="set-detail">a command for Claude Code or Codex</span>${SET_CHEVRON}</button></form></div>
<p class="set-note">An agent is Claude Code or Codex on one of your computers, connected by the Trommi connector. Its sessions stand under it; a session's helpers are counted beside it.</p>`, { lead: groups.length ? `${groups.length} ${groups.length === 1 ? 'agent' : 'agents'} on ${computers} ${computers === 1 ? 'computer' : 'computers'} · ${n} ${n === 1 ? 'session' : 'sessions'}` : '' })
}

/**
 * What Delete of session `a` takes along: `all`, it and the helpers its own agent runs under it (another agent's main
 * session the human put under it is no helper: it stays, and so does a session on another desk or in the archive);
 * `devices`, the agent devices that carried only those (not this device, no person's device). me: this device's id;
 * human(d): whether d is a person's device. m: the board model (everyone: every session, of every desk).
 */
export function deletionOf(m, a, { me = null, human = () => false } = {}) {
  const all = [a, ...m.everyone.filter(x => x.id !== a.id && x.parent === a.id && (!x.main || x.agent_device_id === a.agent_device_id))]
  const ids = new Set(all.map(x => x.id))
  // (a device that also carries a session that stays, on any desk, is left in the room)
  const stays = d => m.everyone.some(x => !ids.has(x.id) && !x.removed && (!x.archived || x.online) && x.agent_device_id === d)
  const devices = [...new Set(all.map(x => x.agent_device_id).filter(Boolean))].filter(d => d !== me && !human(d) && !stays(d))
  return { all, devices }
}

export function register(t) {
  const { BASE, hub } = t
  const members = () => hub.client?.model?.members ?? new Map()
  const deskName = a => { const m = t.model(), id = m.deskOf ? m.deskOf(a) : a.desk; return (m.state?.desks ?? []).find(d => d.id === id)?.name || 'Personal' }
  const groups = () => agentsOf(t.model().everyone ?? t.model().agents ?? [], members(), { me: hub.client?.model?.room?.my_device_id ?? null })
  const show = (req, res, error = '', code = 200) => t.page(req, res, { title: 'Agents · Settings · Trommi', view: 'settings-agents', css: 'agents', main: agentsMain(groups(), BASE, deskName, { error }) }, code)
  t.get(/^\/settings\/agents$/, ({ req, res }) => show(req, res))
  t.get(/^\/settings\/sessions$/, ({ res }) => t.redirect(res, `${BASE}/settings/agents`))
  // Remove an agent from the account: the agent devices of all its sessions (the core gives the others a new key).
  t.post(/^\/agents\/remove$/, async ({ req, res, form }) => {
    const g = groups().find(x => x.key === String(form.get('key') ?? ''))
    try {
      if (!g?.devices.length) throw new Error('this agent is not on the board any more')
      await hub.client.removeDevices(g.devices)
    } catch (err) { return show(req, res, `Not removed: ${sayError(err, 'the board did not take it')}`, 422) }
    t.redirect(res, `${BASE}/settings/agents`)
  })
  t.live('settings-agents', { take: () => String(agentsMain(groups(), BASE, deskName)), diff: (was, now) => (was !== now ? String(t.stream('refresh')) : '') })
  // The drawings of one session's picker: a frame, fetched when the picker is opened.
  t.get(/^\/sessions\/([^/]+)\/marks$/, ({ req, res, url, match }) => {
    const a = t.model().byAgent.get(decodeURIComponent(match[1]))
    if (!a) return t.notFound(req, res, 'This session is not on the board any more.')
    const q = url.searchParams
    t.page(req, res, { title: `Drawing of ${a.name} · Trommi`, view: 'marks', sidebar: false, stream: null, css: 'agents', main: html`<main class="mark-picker t-marks-page">${marksFrame(a, BASE, { stay: q.has('stay'), back: backOf(q.get('back')), where: q.get('in') === 's' ? 's' : '' })}</main>` })
  })

  // Only a path of this board is a page to go back to.
  const backOf = path => (typeof path === 'string' && (BASE ? path.startsWith(`${BASE}/`) : /^\/(?!\/)/.test(path)) && !/[\\\r\n]/.test(path) ? path : '')
  // Sessions laid together share a group (the hub stores the name of the group on each).
  const groupOf = (agents, a) => (a.group ? agents.filter(x => x.group === a.group) : [])
  const WAYS = {
    /** label, icon, desk, parent, archived: whichever the form names; the hub's own rules decide. */
    edit(id, form, m) {
      const body = { agent: id }
      // A name that is the one the session gave itself is no name of the human's.
      if (form.has('label')) { const label = form.get('label').trim(); body.label = label === m.byAgent.get(id)?.given ? '' : label }
      if (form.has('icon')) body.icon = form.get('icon')
      if (form.has('desk')) body.desk = form.get('desk')
      if (form.has('parent')) body.parent = form.get('parent') || null
      if (form.has('archived')) body.archived = form.get('archived') === '1'
      return hub.editSession(body)
    },
    /** A new place: directly before or after another session (the sidebar's drag: `before` or `after`), or one place up
     *  or down among its own kind (`dir`: Move Up/Down, Alt+arrows): the mains and lone sessions of its desk and part
     *  (connected or not), or the subs of its main. */
    move(id, form, m) {
      if (form.has('before') || form.has('after')) return hub.editSession({ agent: id, before: form.get('before') || null, after: form.get('after') || null })
      const u = m.units.find(x => x.id === id)
      if (!u) throw new Error(`no agent ${id}`)
      const live = x => x.online || Boolean(x.subs?.some(s => s.online))
      const among = u.parent ? u.parent.subs : m.units.filter(x => !x.parent && live(x) === live(u) && m.deskOf(x.agent) === m.deskOf(u.agent))
      const at = among.indexOf(u), down = form.get('dir') === 'down'
      if (down ? at >= among.length - 1 : at <= 0) return
      return hub.editSession(down ? { agent: id, after: among[at + 1].id } : { agent: id, before: among[at - 1].id })
    },
    star: (id, form) => hub.starSession({ agent: id, starred: form.get('starred') === '1' }),
    /** Lay this session together with another (or with the group the other is in). */
    async pair(id, form) {
      const agents = hub.state().agents, a = agents.find(x => x.id === id), b = agents.find(x => x.id === form.get('with'))
      if (!a || !b) throw new Error('no such session')
      if (a === b || (a.group && a.group === b.group)) return
      // Whoever the session leaves behind alone is on its own again.
      const left = groupOf(agents, a).filter(x => x !== a)
      if (left.length === 1) await hub.editSession({ agent: left[0].id, group: null })
      const group = (groupOf(agents, b).length > 1 && b.group) || `g${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`
      await hub.editSession({ agent: b.id, group })
      await hub.editSession({ agent: a.id, group })
    },
    /** Take this session out of its group; a group of two is none any more. */
    unpair(id) {
      const agents = hub.state().agents, a = agents.find(x => x.id === id)
      if (!a) throw new Error(`no agent ${id}`)
      const group = groupOf(agents, a)
      return Promise.all((group.length <= 2 ? group : [a]).map(x => hub.editSession({ agent: x.id, group: null })))
    },
  }
  const nameOf = (m, id) => m.byAgent.get(id)?.name ?? m.everyone?.find(a => a.id === id)?.name ?? id   // (an archived session is among everyone only)
  t.post(/^\/sessions\/([^/]+)\/(edit|star|pair|unpair|move)$/, async ({ req, res, url, match, form }) => {
    const id = decodeURIComponent(match[1]), stay = form.has('stay') && t.wantsStream(req)
    let error = ''
    const was = t.model().byAgent.get(id)?.desk ?? null   // (for the Undo of a move to another desk)
    try { await WAYS[match[2]](id, form, t.model()) } catch (err) { error = `Not saved: ${sayError(err, 'the board did not take it')}` }
    if (!error) {
      // The live stream brings the change to every page, this one too; the form itself adds only the toast of an archiving.
      if (stay && match[2] === 'edit' && form.has('moved') && form.has('desk')) {
        const to = (t.model().state.desks ?? []).find(d => d.id === form.get('desk'))
        // (from the session's own page: on to the Desk, the session is not on this desk any more; an Undo stays where it is)
        const on = form.has('leave') ? t.stream('visit', `${BASE}/`) : ''
        return t.sendStream(req, res, html`${on}${t.toast({ head: `Moved to ${to?.name || 'Personal'}`, line: nameOf(t.model(), id).replace(/ · [^·]*$/, ''), undo: was ? { action: `${sessionForms({ id }, BASE)}/edit`, fields: { desk: was, moved: '1' } } : null })}`)
      }
      if (stay) return t.sendStream(req, res, match[2] === 'edit' && form.get('archived') === '1' && !form.has('quiet') ? t.toast({ head: 'Archived', line: nameOf(t.model(), id), undo: { action: `${sessionForms({ id }, BASE)}/edit`, fields: { archived: '0' } } }) : '')
      return t.redirect(res, backOf(form.get('back')) || `${BASE}/settings/agents`)
    }
    if (stay) return t.sendStream(req, res, t.toast({ head: 'Not saved', line: error, role: 'alert' }))
    // Not taken: the Agents page, with what went wrong (422: Turbo shows it in place).
    show(req, res, error, 422)
  })

  // Delete a session (its More menu): what the app can already do, one after the other. Its open questions are shredded
  // (as Shred), it and its helpers go to the archive (as Archive), and the agent devices that carried only them are
  // removed from the room (as Remove under Devices). A session on a person's own device is never deleted this way.
  t.post(/^\/sessions\/([^/]+)\/delete$/, async ({ req, res, match }) => {
    const m = t.model(), id = decodeURIComponent(match[1]), a = m.byAgent.get(id)
    const fail = text => (t.wantsStream(req) ? t.sendStream(req, res, t.toast({ head: 'Not deleted', line: text, role: 'alert' })) : t.redirect(res, `${BASE}/chat/${encodeURIComponent(id)}`))
    if (!a) return fail('no such session')
    if (a.own) return fail('this session runs on your own device')
    const client = hub.client, members = client?.model?.members
    const me = client?.model?.room?.my_device_id
    const { all, devices } = deletionOf(m, a, { me, human: d => members?.get(d)?.device_role === 'human' })
    const ids = new Set(all.map(x => x.id))
    try {
      for (const c of m.state.cards) if (ids.has(c.agent) && c.status === 'open' && c.kind !== 'permission') await hub.shred(c.id, '')
      for (const x of all) await hub.editSession({ agent: x.id, archived: true, deleting: true })
      if (devices.length) await client.removeDevices(devices)
    } catch (err) { return fail(sayError(err, 'the board did not take it')) }
    if (!t.wantsStream(req)) return t.redirect(res, `${BASE}/`)
    return t.sendStream(req, res, html`${t.stream('visit', `${BASE}/`)}${t.toast({ head: 'Deleted', line: a.name })}`)
  })
}
