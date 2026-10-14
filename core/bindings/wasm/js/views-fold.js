// The view model's fold for a JavaScript shell: what core/client/src/views/mod.rs `ViewModel::apply` does, in the
// same order, for the JSON of `Effect::Render` (types: views.d.ts). A shell keeps one model and folds every delta in.
// Checked by tests/bindings/views-replay.mjs against the recorded scenes (core/client/tests/conformance/views).

/** The model before the first delta. The core's first render is always whole (`ViewPart::Whole`): it replaces this. */
export const emptyView = () => ({ rev: 0 })

/** Folds `delta` into `view` (a new object; `view` is left as it was). Throws on a delta out of order. */
export function foldView(view, delta) {
  const whole = delta.parts.some(p => p.part === 'whole')
  if (!whole && delta.base !== view.rev) throw new Error(`a view delta out of order: expected base ${view.rev}, got ${delta.base}`)
  let v = { ...view }
  const keyed = (name, id, unit) => {
    const map = { ...(v[name] ?? {}) }
    if (unit == null) delete map[id]
    else map[id] = unit
    if (Object.keys(map).length) v[name] = map
    else delete v[name]
  }
  for (const p of delta.parts) {
    switch (p.part) {
      case 'whole': v = structuredClone(p.model); break
      case 'phase': v.phase = p.phase; if (p.status == null) delete v.status; else v.status = p.status; break
      case 'sidebar': v.sidebar = p.view; break
      case 'desk': v.desk = p.view; break
      case 'session': keyed('sessions', p.id, p.view); break
      case 'card': keyed('cards', p.id, p.view); break
      case 'off': if (p.view == null) delete v.off; else v.off = p.view; break
      case 'notes': v.notes = p.view; break
      case 'settings': v.settings = p.view; break
      case 'account': v.account = p.view; break
      case 'file': keyed('files', p.id, p.view); break
      case 'toasts': if (p.toasts.length) v.toasts = p.toasts; else delete v.toasts; break
      default: throw new Error(`a view part this fold does not know: ${p.part}`)
    }
  }
  v.rev = delta.base + 1
  return v
}
