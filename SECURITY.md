# Security Policy

## Reporting a vulnerability

Please report security issues privately via GitHub's
[security advisories](https://github.com/piekstra/pennymac-cli/security/advisories/new)
rather than a public issue.

## Threat model

`pmac` authenticates to Pennymac's mortgage servicing portal on behalf of one
borrower and reads their loan and financial data. What's worth protecting:

- **The portal password**, stored in the OS keychain under service
  `piekstra.pmac`, account `password`. Read at point of use, never logged,
  never on argv, never written to disk. It is posted to the identity server in
  the site's own `base64(charCode,…)` encoding — an encoding, not encryption;
  treat the stored value as the password itself.
- **The session cookie** (`PMST`, keychain account `session`) and the
  **device-trust cookies** (account `device`). Both are credentials: the
  session cookie reads the account until the portal expires it, and the device
  cookies let a login skip the SMS second factor. They live only in the
  keychain and are redacted from all output.
- **The `user_access` bearer token.** Minted per session from
  `request_token`; held in memory only, never persisted, never logged.

## What this tool does not do

- It never mutates the portal — no payments, autopay changes, saved bank
  accounts, or profile edits. The write surface is catalogued in
  `src/writes.rs` and the `api` passthrough refuses to POST to any of it.
- It talks only to the configured portal host (`mypennymac.pennymac.com`) and
  its identity provider (`identity.pennymac.com`). No telemetry, no third
  parties.
- It hardcodes no secrets. The only non-secret constant is the portal's public
  OAuth `client_id`. `gitleaks` runs in CI over the full history.

## Handling credentials safely

Prefer piping from a password manager over typing:

```console
$ op read 'op://Private/pennymac/password' \
    | pmac auth set-credential --stdin --overwrite
```

`pmac auth logout --forget` removes the password, session cookie, and
device-trust cookies from the keychain and clears the config.
