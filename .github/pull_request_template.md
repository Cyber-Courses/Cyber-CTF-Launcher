## What changes

<!-- What the change does, for the player or the administrator, and why. -->

## How it was tested

<!-- The tests added or run (pnpm test, pnpm test:web, pnpm test:desktop, a real lab…). -->

## Checklist

- [ ] `pnpm format` passes, and so does `pnpm check` (what CI runs; see
      [CONTRIBUTING.md](../CONTRIBUTING.md#the-gates))
- [ ] `CHANGELOG.md` has a line under **Unreleased** (when a user or an administrator would notice)
- [ ] The docs the change makes wrong are updated (README, `docs/`)
- [ ] Generated command types (`src/lib/tauri/generated/`) are committed when a Rust struct changed
- [ ] New dependencies are under a licence `src-tauri/deny.toml` allows
