// The generated view types (core/bindings/wasm/js/views.d.ts, from tests/bindings/views.mjs) hold the core's real
// views: the demo room's golden units are typed with them, under the strictest settings (npx tsc -p tests/bindings).
import type { CardView, DeskView, SessionView, SidebarView, Screen } from '../../core/bindings/wasm/js/views.d.ts'
import sidebar from '../../core/client/tests/golden/demo-sidebar.json' with { type: 'json' }
import desk from '../../core/client/tests/golden/demo-desk.json' with { type: 'json' }
import session from '../../core/client/tests/golden/demo-session-fernly-web.json' with { type: 'json' }
import card from '../../core/client/tests/golden/demo-card-hero.json' with { type: 'json' }

export const views: [SidebarView, DeskView, SessionView, CardView] = [sidebar, desk, session, card]
export const screen: Screen = { screen: 'off', query: 'tokens' }
