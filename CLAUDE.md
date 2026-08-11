# CLAUDE.md

The canonical agent guide is [AGENTS.md](AGENTS.md) — read it first. It has the
layout, the auth model, the conventions, and the portal's traps.

Claude-specific notes:

- **`make verify` is the gate.** Run it before calling a change done; it's
  exactly what CI runs (`fmt-check` + `clippy -D warnings` + tests + smoke).
- **Tests are offline.** No test may touch the network or the keychain. If you
  need new portal data, capture it, scrub it per `tests/fixtures/README.md`,
  and add a fixture.
- **This CLI is read-only, enforced in three places that must agree:**
  `src/writes.rs` (the catalog), the `api` POST guard, and the claims in
  `README.md`/`AGENTS.md`. Change one, change all three.
- **Never call a write endpoint to "check the shape."** The catalog was built
  by reading the portal's JavaScript, deliberately without invoking anything.
  These endpoints move real money out of a real mortgage.
- **Logging in can cost an SMS code**, and codes are rate-limited. The suite is
  offline for that reason — keep it that way, and don't trigger `auth login`
  against the live portal just to test unless you mean to.
- **The password is posted JS-encoded** (`base64(charCode,…)`), and the portal
  answers a rejected login with `200`. If login "succeeds" but reads fail,
  suspect the encoding or a status-code-only success check before anything else.
