# site

The website [trommi.com](https://trommi.com): one page, the imprint, the privacy page and a not-found page, served as
they are from `public/` by the Cloudflare Worker `trommi-com` (`worker.js`: http to https, www.trommi.com to
trommi.com; `/beta` to the iPhone app's public TestFlight link, `vars.TESTFLIGHT_URL` in `wrangler.jsonc`, or to the
install steps on GitHub while that is empty). Nothing is built (the pictures of the apps in `public/images/` are made once by `dev/showcase.mjs`
from real screenshots of the demo room, see its head); Node 22 or newer, no packages. Font licences: `THIRD-PARTY.md`.

```
node site/dev/serve.mjs              # http://127.0.0.1:8910, with the headers of public/_headers
node tests/site/check.mjs            # links, files, headers, worker; no browser (CI: .github/workflows/pr-check.yml)
node tests/site/header.mjs           # the head of every page in headless Chromium
node site/dev/shots.mjs site         # pictures of every page, desktop and phone, light and dark
node site/dev/showcase.mjs RAW       # the apps' pictures (public/images/, .github/readme/) from screenshots in RAW
```

## Deploy

CI delivers the site: `.github/workflows/ship-site.yml`, on a push to main that changed `site/` (or by hand: Actions
-> ship-site -> Run workflow on main), runs the checks and delivers. Its job deliver asks for the head
of main, checks its site once more, runs the pinned wrangler (`.github/tools/wrangler`) `deploy --config
site/wrangler.jsonc` to the Worker `trommi-com` (custom domains trommi.com and www.trommi.com) with the Cloudflare
token of the 1Password Environment (GitHub environment `site`), and then compares the front page trommi.com serves
with `site/public/index.html`. Cloudflare Workers Builds is disconnected from `trommi-com`: nothing else deploys the
site. `Strict-Transport-Security` (a year, with subdomains, no preload) comes from `public/_headers`.
