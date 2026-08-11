# Test fixtures

These JSON files are **scrubbed captures** of the mypennymac portal's own API
responses, cut down to the fragment each parser actually reads. The
`fixture_shapes` tests run the parsers in `src/parse.rs` against them, so a
portal field rename fails a test here instead of silently emptying a column.

Captured 2026-08 from `https://mypennymac.pennymac.com/api/...`.

| File | Source (verb + path) | Parsed by |
| --- | --- | --- |
| `loans.json` | `POST /api/loan/loans` | `parse::loan_summary`, `parse::escrow` |
| `loan_activity.json` | `POST /api/loan/get_loan_activity` | `parse::transactions`, `parse::payments` |
| `get_docs.json` | `POST /api/documents/get_docs` | `parse::documents` |
| `get_messages.json` | `GET /api/messages/get_messages` | `parse::messages` |
| `request_token.json` | `GET /api/account/request_token` | `parse::profile` |
| `check_authen_cookie.json` | `GET /api/session/check_authen_cookie` | (session probe shape) |

## Scrubbing policy

**Structure is preserved exactly; every identifying or financial value is
replaced with an obvious dummy.** What survives is what the tests are about:
the field names, the nesting, key order, the `…T00:00:00` date encoding, and
the portal's own `null`-vs-absent quirks. What must not survive: the real loan
number, borrower name, address, county, email, phone, SSN, IP, access token,
document/transaction ids, investor names/ids, and — above all — the real
dollar amounts (balances, payments, rates). A mortgage capture is dense with
both PII and financials.

Replacements are deliberately unrealistic so a reader can tell at a glance that
a value is fake:

- loan number → `1000000000`; ids → `9000000xx`
- name → `Sample Owner`; username → `sampleuser`; email → `sample@example.com`
- address → `1 Sample St, Sample City, ST 00000`; county → `SAMPLE`
- phone → `0000000000`; SSN last four → `0000`; IP → `0.0.0.0`
- UUIDs → all-zero `00000000-0000-0000-0000-000000000000`
- access token → a `v2:000…` string
- amounts → round dummies (`1500.00`, `190000.00`, `0.05` rate)

The rule is enforced positively by `fixtures_carry_no_real_identifiers` in
`tests/fixture_shapes.rs` — every UUID must be the all-zero dummy, and a
denylist of *patterns* (real-name fragments, `@gmail`, `Bearer`, cookie names)
must not appear. **Extend that test whenever you add a new kind of identifier.**

## Refreshing a fixture

1. Capture the raw response (`pmac api <path> --loan` for a loan-scoped POST,
   or the browser devtools network tab).
2. Cut it to the fragment the parser reads (plus a field or two to prove
   tolerance of extra keys).
3. Apply the scrub map above *consistently* — the same fake loan number in
   every file — then `cargo test`.

**Never commit a raw capture "to scrub later."** A live `user_access` token or
session cookie is a working credential.
