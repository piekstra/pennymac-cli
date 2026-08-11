//! Non-secret settings (`~/.config/pmac/config.json`).
//!
//! The portal password, the identity device-trust cookies, and the cached
//! mypennymac session all live in the OS keychain (service `piekstra.pmac`),
//! never here.

use serde::{Deserialize, Serialize};

/// The mypennymac servicing portal (the OAuth *client*, and the JSON API host).
pub const DEFAULT_BASE_URL: &str = "https://mypennymac.pennymac.com";

/// The Pennymac identity provider (the OAuth *authorization server*): the
/// Rails/Devise app that owns the username/password + MFA login.
pub const DEFAULT_IDENTITY_URL: &str = "https://identity.pennymac.com";

/// The portal's public OAuth client id. This is not a secret — it ships in the
/// portal's own front-end URL and identifies the app, not the user — but it is
/// configurable so a portal rotation doesn't require a rebuild.
pub const DEFAULT_CLIENT_ID: &str = "JoD8tKVbCGvsqJQ1-rm7gLA3OrfrO8xwEriXXiDrcw4";

/// Keychain account: the portal password (stored so an expired session can be
/// re-minted without a fresh MFA challenge, given the device-trust cookies).
pub const KEYCHAIN_ACCOUNT: &str = "password";

/// Keychain account: the cached mypennymac session cookie bundle (`PMST` + the
/// load-balancer cookies). Ordinary reads authenticate with these alone, after
/// exchanging them for a short-lived bearer via `request_token`.
pub const SESSION_ACCOUNT: &str = "session";

/// Keychain account: the identity provider's device-trust cookies
/// (`_secure_pennymacusa_com_tfa` and friends), set when a login checks
/// "remember this device". Replaying them lets a later login skip the SMS
/// second factor — the difference between an unattended re-auth and a code.
pub const DEVICE_ACCOUNT: &str = "device";

/// Keychain account: an in-flight login parked mid-MFA. `auth login` on a
/// non-trusted device sends a code and stores the identity session that
/// requested it here, so `auth login --code <CODE>` resumes *that* session
/// rather than starting a fresh one the code wouldn't match.
pub const PENDING_ACCOUNT: &str = "pending-login";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Override the portal base URL (default [`DEFAULT_BASE_URL`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Override the identity provider URL (default [`DEFAULT_IDENTITY_URL`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_url: Option<String>,

    /// Override the OAuth client id (default [`DEFAULT_CLIENT_ID`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Portal login username (identity label only; secrets stay in the keychain).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Re-authenticate automatically when the portal expires the session,
    /// instead of failing with exit 3. Defaults to on.
    ///
    /// Re-auth reuses the stored password and the device-trust cookies, so it
    /// only works silently on a device that has completed MFA at least once.
    /// Turn it off to require an explicit `auth login`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_login: Option<bool>,
}

impl Config {
    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
    }

    pub fn identity_url(&self) -> String {
        self.identity_url
            .clone()
            .unwrap_or_else(|| DEFAULT_IDENTITY_URL.to_string())
    }

    pub fn client_id(&self) -> String {
        self.client_id
            .clone()
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
    }

    /// Resolve the login username: config, then `$PMAC_USERNAME`.
    pub fn username(&self) -> Option<String> {
        self.username.clone().or_else(|| {
            std::env::var("PMAC_USERNAME")
                .ok()
                .filter(|s| !s.is_empty())
        })
    }

    /// Whether to re-authenticate automatically on session expiry.
    pub fn auto_login(&self) -> bool {
        self.auto_login.unwrap_or(true)
    }
}

/// Config keys settable via `pmac config set <key> <value>`.
pub const KNOWN_KEYS: &[&str] = &[
    "base_url",
    "identity_url",
    "client_id",
    "username",
    "auto_login",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_login_defaults_on_but_is_overridable() {
        assert!(Config::default().auto_login());
        let off = Config {
            auto_login: Some(false),
            ..Default::default()
        };
        assert!(!off.auto_login());
    }

    #[test]
    fn urls_fall_back_to_the_defaults() {
        let c = Config::default();
        assert_eq!(c.base_url(), DEFAULT_BASE_URL);
        assert_eq!(c.identity_url(), DEFAULT_IDENTITY_URL);
        assert_eq!(c.client_id(), DEFAULT_CLIENT_ID);
    }
}
