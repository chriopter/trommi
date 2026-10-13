# Agents

- Read SECURITY.md before you change anything. Do not regress anything it promises, unless the owner clearly agreed first; then update SECURITY.md in the same change.
- No push without asking first.
- Prepare everything in branches first. Finished work, alone or bundled per topic, goes onto a frozen
  `release/<date>-<n>` branch; never commit to `main`.
- `main` changes only by a pull request from that branch: auto-merge, all checks green, squash merge.
- Before handing over: tests green, and `git diff --stat origin/main...HEAD` lists only your files.
- Commit messages are lists: `New: …` / `Updated: …` / `Fixed: …`. No AI attribution anywhere.
- A change people can see gets its line in the pull request's "What's new" section (New:/Better:/Fixed:); internal
  work leaves it empty. The release notes and What's new are made from it. The day's one summary line (and picture)
  goes into `app/web/public/changelog/CHANGELOG.md`, once a day, in its own small pull request.
- Never commit secrets, keys or private e-mail addresses. Docs go into README.md.
