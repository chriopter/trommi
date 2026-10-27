# Agents

- Read SECURITY.md before you change anything. Do not regress anything it promises, unless the owner clearly agreed first; then update SECURITY.md in the same change.
- No push without asking first.
- Work on a branch; never commit to `main` directly. The one machine check is `.github/workflows/check.yml` (cargo check, rustfmt, web tests); builds and releases are made by hand.
- Before handing over: the tests of what you changed run green locally, and `git diff --stat main...HEAD` lists only your files.
- Commit messages are lists: `New: …` / `Updated: …` / `Fixed: …`. No AI attribution anywhere.
- Never commit secrets, keys or private e-mail addresses. Docs go into README.md.
- App logic lives once, in `core/client` (its README, "The app core"). The apps under `app/` only draw and run their
  platform's services and I/O: no decisions, no sorting, no state-dependent texts there. `core/client/tests/shells.rs`
  holds the old logic folders to shrinking only.
