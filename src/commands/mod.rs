//! Domain command modules. Each read emits a `schema`-tagged DTO in `--json`
//! mode and a shaped table/kv view in text mode.

pub mod api;
pub mod autopay;
pub mod documents;
pub mod escrow;
pub mod messages;
pub mod methods;
pub mod payments;
pub mod profile;
pub mod summary;
pub mod transactions;
pub mod writes;

use pk_cli_auth::reauth::with_reauth;
use pk_cli_core::{CliError, CommonArgs};
use pk_cli_secrets::CredentialStore;
use serde_json::Value;

use crate::client::{establish_session, Portal};
use crate::config::{Config, DEVICE_ACCOUNT, KEYCHAIN_ACCOUNT};

// The output contract lives in the shared crate; re-exported so the command
// modules keep their short `use super::{emit, items, table_view}`.
pub use pk_cli_core::output::{rows_of as items, table_view};

/// Portal API paths the reads share. `loan_post` sends `{"loanId": <id>}`.
pub const LOANS: &str = "/api/loan/loans";
pub const LOAN_ACTIVITY: &str = "/api/loan/get_loan_activity";
pub const DOCUMENTS: &str = "/api/documents/get_docs";
pub const MESSAGES: &str = "/api/messages/get_messages";
pub const PAYMENT_INFO: &str = "/api/payment/get_payment_info";
pub const BANK_ACCOUNTS: &str = "/api/payment/get_bank_accounts";

pub struct Ctx<'a> {
    pub common: &'a CommonArgs,
    pub cfg: &'a Config,
    pub creds: &'a CredentialStore,
}

impl Ctx<'_> {
    /// A portal session replayed from the keychain. Expiry surfaces as a
    /// `CliError::Auth` (exit 3) on the first read.
    pub fn client(&self) -> Result<Portal, CliError> {
        Portal::from_cached_session(self.cfg, self.creds)
    }

    /// Run a read against the portal, re-authenticating once if the session
    /// has lapsed and the device is trusted.
    ///
    /// **Reads only.** The retry rails live in `pk_cli_auth::reauth`; a fresh
    /// client is built per attempt so the retry picks up the session that
    /// `relogin` just wrote to the keychain.
    pub fn read<T>(&self, op: impl Fn(&Portal) -> Result<T, CliError>) -> Result<T, CliError> {
        with_reauth(|| op(&self.client()?), || self.relogin())
    }

    /// Mint a fresh session from the stored password + device-trust cookies.
    fn relogin(&self) -> Result<(), CliError> {
        let username = self.cfg.username();
        let password = self.creds.get(KEYCHAIN_ACCOUNT)?;
        let has_device = self.creds.get(DEVICE_ACCOUNT)?.is_some();
        if let Some(reason) = relogin_blocked(
            self.cfg.auto_login(),
            username.as_deref(),
            password.is_some(),
            has_device,
        ) {
            return Err(CliError::Auth(reason));
        }
        let (username, password) = (username.expect("checked"), password.expect("checked"));
        if !self.common.quiet {
            eprintln!("session expired — re-authenticating as {username}");
        }
        establish_session(self.cfg, self.creds, &username, &password)
    }
}

/// Why an automatic re-login can't proceed, if it can't.
///
/// Split from the action so the policy is testable without a keychain, and so
/// every refusal names the one command that fixes it. Unattended re-auth needs
/// the device-trust cookies too: without them the portal would demand a fresh
/// SMS code that no non-interactive run can answer.
fn relogin_blocked(
    auto_login: bool,
    username: Option<&str>,
    has_password: bool,
    has_device: bool,
) -> Option<String> {
    if !auto_login {
        return Some("portal session expired — run `pmac auth login` (auto_login is off)".into());
    }
    if username.is_none() {
        return Some(
            "portal session expired and no username is configured — run \
             `pmac config set username <you>`, then `pmac auth login`"
                .into(),
        );
    }
    if !has_password {
        return Some(
            "portal session expired and no password is stored — run `pmac auth login`".into(),
        );
    }
    if !has_device {
        return Some(
            "portal session expired and this device isn't remembered — run \
             `pmac auth login` (it needs a verification code)"
                .into(),
        );
    }
    None
}

/// Emit a DTO, taking the `--json` flag off the context.
pub fn emit(ctx: &Ctx, schema: &str, payload: Value, text: impl FnOnce(&Value)) {
    pk_cli_core::output::emit(ctx.common.json, schema, payload, text)
}

/// Print a "nothing here" line to stderr, so stdout stays a clean data stream.
pub fn note_empty(ctx: &Ctx, what: &str) {
    if !ctx.common.quiet {
        eprintln!("no {what} found");
    }
}

/// Apply the shared range flags to a list of DTO rows: keep those whose
/// `date_key` falls in the `--since`/`--until` window (inclusive; ISO dates
/// compare lexically), then cap at `--limit`. A row with no/absent date is not
/// filtered out by a bound, only by the limit.
pub fn paginate(rows: Vec<Value>, date_key: &str, range: &pk_cli_utility::RangeArgs) -> Vec<Value> {
    let keep = |row: &Value| {
        let date = row.get(date_key).and_then(Value::as_str);
        let after = range
            .since
            .as_deref()
            .zip(date)
            .map(|(s, d)| d >= s)
            .unwrap_or(true);
        let before = range
            .until
            .as_deref()
            .zip(date)
            .map(|(u, d)| d <= u)
            .unwrap_or(true);
        after && before
    };
    rows.into_iter()
        .filter(keep)
        .take(range.limit.map(|n| n as usize).unwrap_or(usize::MAX))
        .collect()
}

#[cfg(test)]
mod paginate_tests {
    use super::paginate;
    use pk_cli_utility::RangeArgs;
    use serde_json::json;

    fn rows() -> Vec<serde_json::Value> {
        vec![
            json!({ "date": "2026-08-05" }),
            json!({ "date": "2026-07-05" }),
            json!({ "date": "2026-06-05" }),
        ]
    }

    #[test]
    fn since_and_until_are_inclusive() {
        let r = RangeArgs {
            since: Some("2026-07-01".into()),
            until: Some("2026-08-05".into()),
            limit: None,
        };
        let out = paginate(rows(), "date", &r);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["date"], json!("2026-08-05"));
    }

    #[test]
    fn limit_caps_after_filtering() {
        let r = RangeArgs {
            since: None,
            until: None,
            limit: Some(1),
        };
        assert_eq!(paginate(rows(), "date", &r).len(), 1);
    }

    #[test]
    fn no_flags_keeps_everything_and_a_custom_date_key_is_honored() {
        assert_eq!(paginate(rows(), "date", &RangeArgs::default()).len(), 3);
        let received = vec![json!({ "received": "2026-05-19" })];
        let r = RangeArgs {
            since: Some("2026-06-01".into()),
            until: None,
            limit: None,
        };
        assert!(paginate(received, "received", &r).is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relogin_proceeds_when_everything_is_in_place() {
        assert_eq!(relogin_blocked(true, Some("me"), true, true), None);
    }

    #[test]
    fn relogin_declines_when_auto_login_is_off() {
        let why = relogin_blocked(false, Some("me"), true, true).expect("blocked");
        assert!(why.contains("auto_login is off"), "{why}");
        assert!(why.contains("pmac auth login"), "{why}");
    }

    #[test]
    fn auto_login_off_takes_precedence() {
        let why = relogin_blocked(false, None, false, false).expect("blocked");
        assert!(why.contains("auto_login is off"), "{why}");
    }

    #[test]
    fn relogin_declines_without_username_password_or_device() {
        assert!(relogin_blocked(true, None, true, true)
            .unwrap()
            .contains("config set username"));
        assert!(relogin_blocked(true, Some("me"), false, true)
            .unwrap()
            .contains("no password is stored"));
        let no_dev = relogin_blocked(true, Some("me"), true, false).unwrap();
        assert!(no_dev.contains("isn't remembered"), "{no_dev}");
        assert!(no_dev.contains("verification code"), "{no_dev}");
    }
}
