# Agents

- Read SECURITY.md before you change anything. Do not regress anything it promises, unless the owner clearly agreed first; then update SECURITY.md in the same change.
- No push without asking first.
- Prepare everything in branches first; finished work goes to `staging`, never to `main`.
- `main` changes only by a pull request from a frozen `release/<date>-<n>` branch: all checks green, squash merge.
- Before handing over: tests green, and `git diff --stat origin/staging...HEAD` lists only your files.
- Commit messages are lists: `New: …` / `Updated: …` / `Fixed: …`. No AI attribution anywhere.
- A change people can see gets one line in CHANGELOG.md.
- Never commit secrets, keys or private e-mail addresses. Docs go into README.md.
