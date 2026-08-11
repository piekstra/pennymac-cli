# Pennymac — observed API

Reverse-engineered from the mypennymac portal's own XHR traffic at
`mypennymac.pennymac.com` and its identity provider `identity.pennymac.com`,
2026-08. **Unofficial and undocumented** — any of this can change without
notice. `pmac api <path>` is the escape hatch for checking a shape by hand.

## How this was mapped

The portal is a React single-page app that talks to a JSON API under
`/api/…` on its own host, and delegates login to a separate Rails/Devise
identity server over OAuth. Everything below came from watching the network
tab during a real login and dashboard load, then replaying calls with `curl`
to confirm exactly which credentials each endpoint needs. Nothing was inferred
from the rendered markup.

## Authentication

Auth is a two-host OAuth authorization-code flow, and reads use **two**
credentials layered on top of it:

- **`identity.pennymac.com`** — the OAuth authorization server. Owns the
  username, password, SMS/email second factor, and device trust.
- **`mypennymac.pennymac.com`** — the OAuth client and the API host. Its
  session cookie `PMST` is the durable credential.

### The login flow

| Step | Request | Notes |
| --- | --- | --- |
| 1 | `GET identity…/oauth/authorize?response_type=code&client_id=…&redirect_uri=…%2Fcallback` | 302 → sign-in page. `client_id` is public. |
| 2 | `GET identity…/users/sign_in` | Scrape the CSRF token from `<meta name="csrf-token">`. |
| 3 | `POST identity…/users/validate` | Form: `authenticity_token`, `user[user_name]`, `user[password]` (**encoded — see trap**), `user[remember_device]=true`. |
| 3a | *(new device)* → `GET /users/mfa` | A code is sent; see MFA below. |
| 3b | *(trusted device)* → 302 chain through `/users/login` → `/oauth/authorize` → `mypennymac…/callback` | Sets `PMST`. Done. |

### The encoded-password trap

`user[password]` is **not** the raw password. The sign-in page's JavaScript
transforms it before posting — from the bundle:

```js
function m(e){var t=[];for(i=0;i<e.length;i++)t.push(e.charCodeAt(i));return window.btoa(t.toString())}
```

i.e. `base64(charCode,charCode,…)`. Posting the raw password authenticates
against nothing and the server re-renders the sign-in page with a `200` — a
client that trusts the status code reports a login that never happened.
`src/client.rs::encode_password` reproduces `m()` exactly.

### MFA and device trust

On a device the identity server doesn't recognize, `POST /users/validate`
lands on `/users/mfa` and an SMS code goes out. The code is verified with:

- `POST identity…/users/mfa/request_verification` — `{ "mfa_type": "sms", "resend": false }` (re)sends a code.
- `POST identity…/users/mfa/verify` — `{ "mfa_type": "sms", "remember_me": true, "verification_token": "<code>" }`.

`remember_me: true` is what sets the device-trust cookies
(`_secure_pennymacusa_com_tfa` and a set of `ft*` cookies on the identity
host). **Replaying those cookies on a later login skips MFA entirely** — this
is the difference between an unattended re-auth and one that needs a phone.
`pmac` stores them (keychain account `device`) and seeds them on every login.

Because a code is single-use and expires in minutes, `pmac auth login` on a
non-TTY parks the in-flight identity session (keychain account `pending-login`)
and exits asking for `pmac auth login --code <CODE>`, which resumes *that*
session rather than starting a fresh one the code wouldn't match.

### The session + bearer, on every read

Once logged in, reads never touch the identity host. They:

1. Replay the `PMST` cookie to `GET /api/account/request_token` — which
   returns `is_logged_in`, the `loan_numbers`, and a **`user_access`** field:
   an ~1180-char bearer token, minted fresh per session.
2. Send `Authorization: Bearer <user_access>` (plus the `PMST` cookie) on the
   data endpoints.

`request_token` needs only the cookie; the data endpoints need the cookie
**and** the bearer. `pmac` mints the bearer once per process and reuses it.

Cookies worth naming: `PMST` (session, httpOnly, mypennymac) and the
`AWSALB`/`AWSALBCORS` load-balancer pair. `pmac` persists all three (keychain
account `session`) and writes them back if the portal rotates them.

### Session expiry

The portal does **not** answer `401` for an expired session. The tells, all of
which `pmac` treats as auth failure (exit 3):

- `request_token` returns `is_logged_in: false`.
- A data call redirects to the `identity.` host (bounced to login).
- An HTML body arrives where JSON was expected (`parse_json` catches the
  leading `<`).

## Reads

All under `mypennymac.pennymac.com`. POSTs take `{"loanId": <your loan>}`
unless noted.

| Path | Method | Returns | Used by |
| --- | --- | --- | --- |
| `/api/session/check_authen_cookie` | GET | `{hasCookie, hasAuth0Cookie}` | session probe |
| `/api/account/request_token` | GET | profile + `user_access` bearer + `loan_numbers` | `profile`, session bootstrap |
| `/api/loan/loans` | POST | `{loanSummary:[{…}]}` | `summary`, `balance`, `escrow` |
| `/api/loan/get_loan_activity` | POST | `[{loanId, history:[…], …}]` | `transactions`, `payments` |
| `/api/documents/get_docs` | POST | `{whiteListedDoc:[{…}]}` | `documents` |
| `/api/messages/get_messages` | GET | `[{messageId, body, …}]` | `messages` |
| `/api/payment/get_payment_info` | POST | ACH status, scheduled payment, funding account, pending payments | `autopay` |
| `/api/payment/get_bank_accounts` | POST | `[{accountNickName, accountName, routingTransitNumber, accountNumber, …}]` | `methods` |

### Shapes worth knowing

- **Money is a bare JSON float** (`1234.56`), not cents and not a string.
  `pmac` renders it as a string-decimal `Money` DTO; never compare the raw
  floats for equality.
- **Dates are `"YYYY-MM-DDT00:00:00"`** (midnight, sometimes with a fractional
  second on `uploaded`). `src/dates.rs` trims to the date; `0001-01-01` and
  `1900-01-01` are "never" sentinels that read as absent.
- **`loanSummary[0]` has ~360 fields.** The escrow balance and UPB live in a
  nested `balanceSummary` object *and* (sometimes) at the top level; `pmac`
  prefers the nested one and falls back.
- The ledger lives at `get_loan_activity[0].history[]`; a borrower payment is a
  row with `transactionType == "Payment"`.
- **Bank-account payloads are dense with sensitive fields** — raw
  `routingTransitNumber`, an `unmaskedAccountNumberEncode` blob, `accountName`,
  and SSN fragments — while `accountNumber` is already masked to the last four.
  `methods`/`autopay` surface the account holder, routing, and masked account
  (this is your own data in your own CLI); they skip only the useless bits (the
  encoded blob, internal ids, and the opaque numeric `bankAccountType`, which
  is *not* a checking/savings flag). `bankAccountType` has been seen as a large
  code like `201286`, so never render it as an account type.

## Writes — catalogued, not implemented

`pmac` is read-only. The portal's mutating endpoints (payments, autopay, saved
bank accounts, escrow-shortage elections, profile edits) are catalogued in
`src/writes.rs` and printed by `pmac writes`. **None have been called** — the
paths were read from the front-end code, not exercised, and `pmac api` refuses
to POST to any of them. On a mortgage, these move real money; verify each one
against live traffic before ever implementing it.
