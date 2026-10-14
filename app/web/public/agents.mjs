// Settings · Agents (<base>/settings/agents): the agents (a program on a computer: "Claude Code on MacBook Pro") with
// their main sessions as the branches of a tree, as the app core lists them (vm.settings.agent_list, AgentView). An
// agent's "…" removes it from the account (AccountAction RemoveDevices with the devices the core names).
// The routes of a session's forms (<base>/sessions/<id>/…) are here too: a session's own page and the sidebar use them;
// each sends the core's Action.
import { PLUS, SET_CHEVRON, avatar, agoSpan, html, marksFrame, sessionForms, settingsPage, sk, usageReset, whoOf } from './ui.mjs'

const dot = on => html`<i class="set-dot" data-online="${on ? 'yes' : 'no'}"></i>`
const hot = m => (m.hot ? ' is-hot' : '')
/** An agent's account: plan and e-mail, a row per meter, Codex's facts (UsageView). */
const usage = u => (u ? html`<div class="use-full"><p class="use-who">${u.plan ? html`<span class="use-plan">${u.plan}</span>` : ''}${u.email ? html`<span class="use-mail">${u.email}</span>` : ''}</p>${(u.meters ?? []).map(m => html`<div class="use-row"><span class="use-l">${m.long}</span><span class="use-bar${hot(m)} is-wide" title="${m.used} % used"><i style="--used:${m.used}%"></i></span><b class="${hot(m).trim()}">${m.used} %</b><small>${m.reset_ms ? `resets ${usageReset(m.reset_ms)}` : ''}</small></div>`)}${u.facts?.length ? html`<p class="use-facts">${u.facts.map(f => html`<span class="use-fact">${f.en}</span>`)}</p>` : ''}</div>` : '')
function agentCard(g, i, base) {
  const name = html`${g.program} <span>on ${g.host || 'an unnamed computer'}</span>`
  return html`<section class="set-group ag-card" id="agent-${i}" aria-label="${g.name.en}">
<div class="ag-head"><span class="ag-head-ico">${sk('terminal')}</span><span class="ag-head-words"><b>${name}</b><small>${dot(g.online)}${g.state.en}${g.online || !g.seen_ms ? '' : html` ${agoSpan(g.seen_ms)}`} · ${g.count.en}</small></span>
${g.remove && g.devices?.length ? html`<details class="ag-more"><summary title="More" aria-label="More for ${g.name.en}">${sk('more')}</summary><form method="post" action="${base}/agents/remove" class="ag-menu"><input type="hidden" name="key" value="${g.key}"><p>${g.remove.en}</p><button type="submit" class="room-danger">Remove from This Account</button></form></details>` : ''}</div>
${usage(g.usage)}<ul class="ag-tree">${g.sessions.map(a => html`<li><a class="ag-sess" href="${base}/chat/${encodeURIComponent(a.slug)}" data-nav>${avatar(whoOf(a), { crown: false })}<span class="set-two"><b>${a.name.en}</b><small>${a.desk.en}${a.folder ? html` · <span class="room-mono">${a.folder}</span>` : ''}</small></span>${a.helpers_says ? html`<span class="ag-helpers">${a.helpers_says.en}</span>` : html`<span></span>`}<span class="set-detail">${dot(a.online)}${a.state.en}</span>${SET_CHEVRON}</a></li>`)}</ul></section>`
}
/** The page's main (SettingsView). */
export function agentsMain(set, base, { error = '' } = {}) {
  const list = set.agent_list ?? []
  return settingsPage('Agents', html`${error ? html`<p class="room-error" role="alert">${error}</p>` : ''}<div id="agents-list">${list.length ? list.map((g, i) => agentCard(g, i, base)) : html`<div class="set-group"><p class="set-empty">No agent yet.</p></div>`}</div>
<div class="set-group"><form method="post" action="${base}/pair" class="set-agent"><input type="hidden" name="role" value="agent"><button type="submit" class="set-row" id="agents-invite"><span class="set-ico">${PLUS}</span><b>Invite an Agent…</b><span class="set-detail">a command for Claude Code or Codex</span>${SET_CHEVRON}</button></form></div>
<p class="set-note">An agent is Claude Code or Codex on one of your computers, connected by the Trommi connector. Its sessions stand under it; a session's helpers are counted beside it.</p>`, { lead: set.agents_lead?.en ?? '' })
}

export function register(t) {
  const { BASE } = t
  const show = (req, res, error = '', code = 200) => t.page(req, res, { title: 'Agents · Settings · Trommi', view: 'settings-agents', main: agentsMain(t.vm().settings, BASE, { error }) }, code)
  t.get(/^\/settings\/agents$/, ({ req, res }) => show(req, res))
  t.get(/^\/settings\/sessions$/, ({ res }) => t.redirect(res, `${BASE}/settings/agents`))
  // Remove an agent from the account: the agent devices the core names for it.
  t.post(/^\/agents\/remove$/, ({ req, res, form }) => {
    const g = (t.vm().settings.agent_list ?? []).find(x => x.key === String(form.get('key') ?? ''))
    if (!g?.devices?.length) return show(req, res, 'Not removed: this agent is not on the board any more', 422)
    t.account({ step: 'remove_devices', device_ids: g.devices })
    t.redirect(res, `${BASE}/settings/agents`)
  })

  // ---- a session's forms ----
  const rowOf = id => t.vm().sidebar?.sessions?.find(x => x.id === id) ?? null
  const sessionOf = id => t.vm().sessions?.[id] ?? rowOf(id)
  const nameOf = id => sessionOf(id)?.name?.en ?? ''
  const deskName = id => t.vm().sidebar?.desks?.find(d => d.id === id)?.name?.en || 'Personal'
  // The drawings of one session's picker: a frame, fetched when the picker is opened.
  t.get(/^\/sessions\/([^/]+)\/marks$/, ({ req, res, url, match }) => {
    const s = sessionOf(decodeURIComponent(match[1]))
    if (!s) return t.notFound(req, res, 'This session is not on the board any more.')
    const q = url.searchParams
    t.page(req, res, { title: `Drawing of ${s.name.en} · Trommi`, view: 'marks', sidebar: false, main: html`<main class="mark-picker t-marks-page">${marksFrame({ id: s.id, mark: s.mark }, BASE, { stay: q.has('stay'), back: backOf(q.get('back')), where: q.get('in') === 's' ? 's' : '' })}</main>` })
  })
  // Only a path of this board is a page to go back to.
  const backOf = path => (typeof path === 'string' && /^\/(?!\/)/.test(path) && !/[\\\r\n]/.test(path) ? path : '')
  const undo = (id, fields) => ({ action: `${sessionForms({ id }, BASE)}/edit`, fields })

  /** label, icon, desk, parent, group, archived: whichever the form names, each its Action. Returns the stream to answer with. */
  function edit(id, form) {
    if (form.has('label')) t.act({ do: 'rename_session', session_id: id, name: String(form.get('label')).trim() })
    if (form.has('icon')) t.act({ do: 'session_icon', session_id: id, icon: String(form.get('icon')) || null })
    if (form.has('parent')) t.act({ do: 'session_parent', session_id: id, parent: String(form.get('parent')) || null })
    if (form.has('group')) t.act({ do: 'session_group', session_id: id, group: String(form.get('group')) || null })
    if (form.has('desk')) {
      const was = sessionOf(id)?.desk_id, to = String(form.get('desk'))
      t.act({ do: 'move_session', session_id: id, desk_id: to })
      // (from the session's own page: on to the Desk, the session is not on this desk any more)
      if (form.has('moved')) return html`${form.has('leave') ? t.stream('visit', `${BASE}/`) : ''}${t.toast({ head: `Moved to ${deskName(to)}`, line: nameOf(id), undo: was ? undo(id, { desk: was, moved: '1' }) : null })}`
    }
    if (form.has('archived')) {
      const on = form.get('archived') === '1'
      t.act({ do: 'archive_session', session_id: id, on })
      if (on && !form.has('quiet')) return t.toast({ head: 'Archived', line: nameOf(id), undo: undo(id, { archived: '0' }) })
    }
    return ''
  }
  /** A new place: directly before or after another session (the sidebar's drag), or one up or down among the rows of
   *  its part of the list (`dir`: Move Up/Down). */
  function move(id, form) {
    if (form.has('before') || form.has('after')) return t.act({ do: 'place_session', session_id: id, before: form.get('before') || null, after: form.get('after') || null })
    // CORE GAP: no action moves a session one place up or down; its neighbour is read off the sidebar's rows.
    const rows = t.vm().sidebar?.sessions ?? [], me = rows.find(x => x.id === id)
    if (!me) return
    const among = rows.filter(x => (me.parent ? x.parent === me.parent : !x.parent && x.group === me.group)), at = among.indexOf(me)
    const next = among[form.get('dir') === 'down' ? at + 1 : at - 1]
    if (next) t.act({ do: 'place_session', session_id: id, ...(form.get('dir') === 'down' ? { after: next.id } : { before: next.id }) })
  }
  t.post(/^\/sessions\/([^/]+)\/(edit|star|move)$/, ({ req, res, match, form }) => {
    const id = decodeURIComponent(match[1]), stay = form.has('stay') && t.wantsStream(req)
    let said = ''
    if (match[2] === 'edit') said = edit(id, form)
    else if (match[2] === 'star') t.act({ do: 'crown_session', session_id: id, on: form.get('starred') === '1' })
    else move(id, form)
    if (stay) return said ? t.sendStream(req, res, said) : t.done(res)
    t.redirect(res, backOf(form.get('back')) || `${BASE}/settings/agents`)
  })
  // Delete a session (its More menu): the core shreds its open questions, archives it with its helpers and removes
  // the agent devices that carry nothing else.
  t.post(/^\/sessions\/([^/]+)\/delete$/, ({ req, res, match }) => {
    const id = decodeURIComponent(match[1]), name = nameOf(id)
    t.act({ do: 'delete_session', session_id: id })
    if (!t.wantsStream(req)) return t.redirect(res, `${BASE}/`)
    t.sendStream(req, res, html`${t.stream('visit', `${BASE}/`)}${t.toast({ head: 'Deleted', line: name })}`)
  })
}
