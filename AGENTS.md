# AGENTS.md — pennymac-cli

The canonical guide for working in this repo. `pmac` is a piekstra-family CLI
(spec **piekstra-cli/1**) over Pennymac's mortgage servicing portal. Rust, clap,
keychain-secured, self-updating. **Read-only.**

## Layout

| Path | What it owns |
| --- | --- |
| `src/main.rs` | clap tree, `auth`/`config`/`self-update`/`completions`/`info`, exit-code mapping |
| `src/client.rs` | the `Portal`: OAuth login + MFA, `request_token` bearer, data calls, expiry detection |
| `src/config.rs` | non-secret settings + keychain account names |
| `src/dates.rs` | portal `…T00:00:00` → ISO `YYYY-MM-DD` |
| `src/parse.rs` | pure JSON → DTO mappers (the tested core) |
| `src/writes.rs` | catalog of mutating endpoints (never called) |
| `src/commands/*.rs` | one module per command group |
| `tests/` | offline black-box + fixture-contract tests |
| `docs/api.md` | the reverse-engineered endpoints **and the traps** |

Built on the shared `pk-cli-*` crates from
[`cli-common`](https://github.com/piekstra/cli-common), pinned to a tag. Reuse
those (`output`, `dates`, `Money`, `CredentialStore`, `AuthStatus`,
`RangeArgs`, `reauth`) before writing a helper.

## The auth model (the part that surprises people)

Two hosts, OAuth between them; reads use a cookie **and** a bearer. The full
map is in [`docs/api.md`](docs/api.md); the load-bearing facts:

- The login password is posted **base64(charCode,…)-encoded**, not raw — the
  site's own JS transform, reproduced in `client.rs::encode_password`. Raw
  fails with a `200`.
- First login needs an SMS code; `remember_me` then stores device-trust cookies
  (keychain account `device`) that make later logins skip MFA. Automatic
  re-auth only works when those are present.
- Every read exchanges the `PMST` cookie for a fresh `user_access` bearer via
  `GET /api/account/request_token`, then sends `Authorization: Bearer …` on the
  data endpoints.
- Session expiry is never a `401`: it's `is_logged_in:false`, a redirect to the
  identity host, or HTML where JSON was due. All three map to exit 3.

## Non-negotiable conventions

- **`--json` on every command**, one `schema`-tagged DTO on stdout; diagnostics
  to stderr. Exactly one JSON document per invocation.
- **Exit codes:** 0 ok · 2 usage · 3 auth · 4 not found · 5 upstream · 6
  confirmation required.
- **Validate args before touching keychain or network** — `--help` and bad
  input must never prompt, hang, or hit the portal.
- **Secrets only** from the keychain (`piekstra.pmac`), stdin, or a named env
  var — never argv, never a file, never a log. Full account numbers and tokens
  never appear in `--verbose`.
- **ISO `YYYY-MM-DD`** on every date flag; the portal's `…T00:00:00` stays
  internal.
- **Money is a string decimal** (`{"amount":"…","currency":"USD"}`), never a
  float — even though the portal sends bare floats.

## Read-only is enforced in three places

`src/writes.rs` (the catalog), the `api` POST guard, and the claims in
`README.md`/this file. They must stay in agreement — changing one means changing
all three. The catalog was built by reading the portal's front-end code
**without invoking anything**; these endpoints move real money out of a real
mortgage. Don't call one "to check the shape."

## Tests are offline, always

`cargo test`, no network, no keychain:

- `tests/cli_surface.rs` — black-box `assert_cmd`: every command's `--help`
  renders, exit codes, the write-guard, range validation, `info`, completions.
- `tests/fixture_shapes.rs` — the `parse::*` mappers over scrubbed captures in
  `tests/fixtures/`, so a portal field rename fails loudly; plus the positive
  scrub-enforcement test.
- Unit tests inline per module.

**Fixtures are scrubbed captures** — structure preserved exactly, every
identifying or financial value replaced with an obvious dummy. See
`tests/fixtures/README.md`; a test enforces it. Never commit a raw capture: a
`user_access` token or session cookie is a live credential.

## Dogfood before declaring done

`make verify` is necessary, not sufficient. Install and drive the real binary
across the surface — every command, `--json`, the error paths — since that is
what finds clap panics, provider quirks, and lying success messages.
**Reinstall after every fix** (`make install`): a stale `~/.cargo/bin/pmac`
serves old behavior. Testing against the live portal can cost an SMS code, so
keep the suite offline and don't burn codes casually.

## Definition of done

`make verify` green, CI green, the change dogfooded through the **installed**
binary, `--json` and human output in sync, `docs/api.md` matching reality, and
no secrets or personal data anywhere in the diff — fixtures included.
