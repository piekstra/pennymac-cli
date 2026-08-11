# pennymac-cli (`pmac`)

Your [Pennymac](https://www.pennymac.com/) mortgage servicing portal
(`mypennymac.pennymac.com`) from the command line — balance, escrow, statements,
transactions, and documents, as JSON or tables, Keychain-secured.

**Read-only.** Every command observes; none pays, enrolls, or edits anything.
The portal's write surface is catalogued (`pmac writes`) but deliberately
unimplemented.

> Unofficial. Not affiliated with or endorsed by PennyMac. It talks to the same
> undocumented endpoints the website's own front end uses (see
> [`docs/api.md`](docs/api.md)), so a portal redesign can break it.

Conforms to the **piekstra-cli/1** spec: `--json` on every command, standard
`auth` / `config` / `self-update` / `completions` / `info`, keychain-only
secrets, ISO dates, and family exit codes.

## Install

```console
$ git clone https://github.com/piekstra/pennymac-cli
$ cd pennymac-cli
$ make install      # cargo install + macOS re-sign so keychain grants persist
```

Requires a Rust toolchain. On macOS the binary is re-signed with a stable local
identity so the Keychain "Always Allow" grant survives reinstalls.

## Log in

The username lives in config; the password lives in the keychain. Pipe it
straight from your password manager — never type it on the command line:

```console
$ pmac config set username <your-username>
$ op read 'op://Private/pennymac/password' | pmac auth set-credential --stdin
$ pmac auth login
```

The first login on a machine needs a one-time SMS code (the portal's second
factor):

```console
$ pmac auth login
A verification code was sent to your phone/email on file.
Verification code: 123456
```

`--remember` is implied: the device is trusted afterward, so later logins — and
automatic re-auth when the session lapses — skip the code. On a headless box,
`pmac auth login` sends the code and exits; resume with
`pmac auth login --code <CODE>`.

## Use

```console
$ pmac summary                     # amount due, due date, balances, rate, property
$ pmac balance                     # same view, balance-first
$ pmac escrow                      # escrow balance, monthly components, tax disbursements
$ pmac transactions list --limit 10
$ pmac payments list --since 2026-01-01
$ pmac documents list              # statements, escrow analyses, 1098s (alias: statements)
$ pmac messages list
$ pmac profile                     # the account holder on file
$ pmac summary --json | jq .amount_due
```

Every command takes `--json` for a schema-tagged DTO on stdout (diagnostics go
to stderr, so pipes stay clean). Lists take `--limit N`, `--since`, and
`--until` (ISO `YYYY-MM-DD`), and each `list` has an `ls` alias.

### Raw API passthrough

```console
$ pmac api /api/session/check_authen_cookie
$ pmac api /api/loan/loans --loan          # inject {"loanId": <your loan>}
```

`api` attaches your session and bearer token automatically, and **refuses to
POST to any endpoint in the write catalog** (`pmac writes`) — the escape hatch
can't move money.

## Read-only, and how that's enforced

`pmac` implements no mutating endpoint. The portal's writes — one-time
payments, autopay, saved bank accounts, escrow-shortage elections, profile
edits — are listed by `pmac writes` and blocked by the `api` guard. See
[`docs/api.md`](docs/api.md) for the full map and [`SECURITY.md`](SECURITY.md)
for the threat model.

## Commands

| Command | What it shows |
| --- | --- |
| `auth login \| status \| logout \| set-credential` | credential + session management |
| `config path \| show \| set \| unset` | non-secret settings |
| `summary` / `balance` | amount due, due date, balances, rate, property |
| `escrow` | escrow balance, monthly components, tax/insurance disbursements |
| `transactions list` | the full loan ledger |
| `payments list` | posted mortgage payments |
| `documents list` (`statements`) | statements, escrow analyses, tax forms |
| `messages list` | message-center notices |
| `profile` | the account holder on file |
| `writes` | the mutating endpoints this CLI won't call |
| `api` | raw read-only passthrough |
| `self-update`, `completions`, `info` | family standard commands |

## Exit codes

`0` ok · `2` usage · `3` auth required/expired · `4` not found · `5` upstream
(portal error) · `6` confirmation required. Drivers can branch on `3`
("re-login") and `5` ("portal issue") without parsing messages.

## Development

```console
$ make verify      # fmt-check + clippy -D warnings + tests + smoke (the CI gate)
$ make test        # offline: no network, no keychain
$ make dev         # debug build, re-signed
```

Tests are fully offline; portal data lives as scrubbed fixtures under
`tests/fixtures/`. See [`CONTRIBUTING.md`](CONTRIBUTING.md) and
[`AGENTS.md`](AGENTS.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.
