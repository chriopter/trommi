// The generated view types (core/bindings/wasm/js/views.d.ts, from tests/bindings/views.mjs) hold the core's real
// views: the demo room's golden units are typed with them, under the strictest settings (npx tsc -p tests/bindings).
import type { CardView, GroupView, SessionView, SidebarView, Screen } from '../../core/bindings/wasm/js/views.d.ts'
import sidebar from '../../core/client/tests/golden/demo-sidebar.json' with { type: 'json' }
import group from '../../core/client/tests/golden/demo-group.json' with { type: 'json' }
import session from '../../core/client/tests/golden/demo-session-fernly-web.json' with { type: 'json' }
import card from '../../core/client/tests/golden/demo-card-hero.json' with { type: 'json' }

// A JSON import types its strings as string, never as a literal: GroupView's ⋯ menu opens a Screen (tagged by its
// `screen`), which such a string cannot be. Wide<T> is T with its string literals widened, its shape kept strictly.
type Wide<T> = T extends string ? string
  : T extends number | boolean | null ? T
  : T extends readonly (infer U)[] ? Wide<U>[]
  : T extends object ? { [K in keyof T]: Wide<T[K]> }
  : T

export const views: [SidebarView, Wide<GroupView>, SessionView, CardView] = [sidebar, group, session, card]
export const screen: Screen = { screen: 'off', query: 'tokens' }
