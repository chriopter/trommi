// The site's worker (wrangler.jsonc: it runs before the static assets of public/).
//   - plain http goes to https;
//   - www.trommi.com goes to trommi.com (one address for the site);
//   - /beta goes to the public TestFlight link of the iPhone app (vars.TESTFLIGHT_URL in wrangler.jsonc), or to the
//     install steps on GitHub while none is set;
//   - everything else is the static site.

/** Where a request is sent instead, or null: http to https, www to the apex (never on a local address). */
export function redirectOf(href) {
  const url = new URL(href)
  if (url.hostname === '127.0.0.1' || url.hostname === 'localhost') return null
  if (url.protocol === 'https:' && url.hostname !== 'www.trommi.com') return null
  url.protocol = 'https:'
  if (url.hostname === 'www.trommi.com') url.hostname = 'trommi.com'
  return url.toString()
}

/** The address of the iPhone beta on the site, and where it goes while no TestFlight link is set. */
export const BETA = '/beta'
export const BETA_FALLBACK = 'https://github.com/trommi/trommi#install'

/** Where /beta goes: the configured TestFlight public link when it is one, else the install steps. */
export function betaOf(link) {
  return /^https:\/\/testflight\.apple\.com\/join\/[A-Za-z0-9]+$/.test(link ?? '') ? link : BETA_FALLBACK
}

export default {
  fetch(request, env) {
    const to = redirectOf(request.url)
    if (to) return Response.redirect(to, 301)
    // (302: the link may change, a browser must not keep it)
    if (new URL(request.url).pathname === BETA) return Response.redirect(betaOf(env.TESTFLIGHT_URL), 302)
    return env.ASSETS.fetch(request)
  }
}
