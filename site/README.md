# site

The website [trommi.com](https://trommi.com): one page, the imprint, the privacy page and a not-found page, served as
they are from `public/` with `worker.js` in front (http to https, www.trommi.com to trommi.com; `/beta` to the iPhone
app's public TestFlight link, the server's setting `TESTFLIGHT_URL`, or to the install steps on GitHub while that is
empty). Nothing is built (the pictures of the apps in `public/images/` are made once by `dev/showcase.mjs`
from real screenshots of the demo room: the web app's group page on a laptop and on a phone, see its head; the
iPhone and Mac pictures come from those apps); Node 22 or newer, no packages. Font licences: `THIRD-PARTY.md`.

```
node site/dev/serve.mjs              # http://127.0.0.1:8910, with the headers of public/_headers
node tests/site/check.mjs            # links, files, headers, worker; no browser
node tests/site/header.mjs           # the head of every page in headless Chromium
node site/dev/shots.mjs site         # pictures of every page, desktop and phone, light and dark
node site/dev/showcase.mjs raw RAW   # screenshots of the web app's demo room into RAW (node app/web/dev/serve.mjs 8900)
node site/dev/showcase.mjs RAW       # the apps' pictures (public/images/, .github/readme/) from screenshots in RAW
```

## Deploy

By hand: run the checks above, copy `public/` (and `worker.js`) to the server and restart its container (root README,
"Releases"). `Strict-Transport-Security` (a year, with subdomains, no preload) comes from `public/_headers`.
