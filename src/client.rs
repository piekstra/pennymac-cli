//! HTTP client for the Pennymac mortgage servicing portal.
//!
//! There is no published API. Everything here targets the endpoints the
//! mypennymac single-page app calls, mapped by watching its XHR traffic. See
//! `docs/api.md`.
//!
//! # Auth model
//!
//! Two hosts, an OAuth authorization-code flow between them:
//!
//! - `identity.pennymac.com` is a Rails/Devise app — the OAuth **authorization
//!   server** that owns the username/password and the SMS second factor.
//! - `mypennymac.pennymac.com` is the OAuth **client** and the JSON API host.
//!
//! `auth login` drives the flow end to end: GET the authorize URL (which
//! redirects to the sign-in page), scrape the Rails CSRF token, then
//! `POST /users/validate` with the username and the password **encoded the way
//! the site's own JavaScript encodes it** — `base64(charCode,charCode,…)`, not
//! the raw string; posting the raw password silently fails. On a device that
//! has passed MFA before (its `_secure_pennymacusa_com_tfa` cookie is present)
//! the validate redirect runs straight through `/oauth/authorize` →
//! `/callback`, and mypennymac sets its `PMST` session cookie. On a new device
//! the validate lands on `/users/mfa` instead; a code is sent and
//! `auth login --code <CODE>` resumes.
//!
//! Ordinary reads never re-run any of that. They replay the cached `PMST`
//! cookie, exchange it via `GET /api/account/request_token` for a short-lived
//! bearer token (the `user_access` field) plus the loan number, and send the
//! bearer on the data endpoints. The cookie is the durable credential; the
//! bearer is minted fresh each run.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use pk_cli_core::CliError;
use pk_cli_secrets::{CredentialStore, Secret};
use reqwest::cookie::CookieStore;
use serde_json::{json, Value};

use crate::config::{Config, DEVICE_ACCOUNT, SESSION_ACCOUNT};

/// A recent desktop Chrome UA. The identity host sits behind bot protection
/// that rejects obviously-scripted clients.
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// mypennymac cookies that constitute a logged-in session. `PMST` is the
/// session token; the `AWSALB*` pair is load-balancer stickiness that the app
/// sends back on every request.
const SESSION_COOKIES: &[&str] = &["PMST", "AWSALB", "AWSALBCORS"];

/// Bearer + loan number, minted once per process from `request_token` and
/// reused across the several data calls a single command makes.
struct TokenCtx {
    bearer: Secret,
    loan_id: String,
}

/// What a login attempt resolved to.
#[derive(Debug, PartialEq, Eq)]
pub enum LoginOutcome {
    /// Fully logged in — the session cookie is set.
    Authenticated,
    /// The portal sent a second-factor code and is waiting for it.
    TwoFactorRequired,
}

/// The second-factor delivery channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CodeChannel {
    Sms,
    Email,
}

impl CodeChannel {
    fn mfa_type(self) -> &'static str {
        match self {
            CodeChannel::Sms => "sms",
            CodeChannel::Email => "email",
        }
    }
}

pub struct Portal {
    http: reqwest::blocking::Client,
    jar: Arc<reqwest::cookie::Jar>,
    base: String,
    identity: String,
    client_id: String,
    token: RefCell<Option<TokenCtx>>,
    /// Keychain write-back for a rotated session bundle.
    sync: Option<SessionSync>,
}

struct SessionSync {
    creds: CredentialStore,
    last: RefCell<String>,
}

impl Portal {
    /// A client with an empty cookie jar and no keychain write-back.
    pub fn new(cfg: &Config) -> Result<Self, CliError> {
        let base = cfg.base_url();
        let identity = cfg.identity_url();
        for u in [&base, &identity] {
            u.parse::<reqwest::Url>()
                .map_err(|e| CliError::Usage(format!("invalid url {u:?}: {e}")))?;
        }
        let jar = Arc::new(reqwest::cookie::Jar::default());
        let http = reqwest::blocking::Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(15))
            .cookie_provider(jar.clone())
            .build()
            .map_err(|e| CliError::Other(format!("failed to build HTTP client: {e}")))?;
        Ok(Portal {
            http,
            jar,
            base,
            identity,
            client_id: cfg.client_id(),
            token: RefCell::new(None),
            sync: None,
        })
    }

    /// Replay a cached session from the keychain. Not verified here; the first
    /// read surfaces expiry.
    pub fn from_cached_session(cfg: &Config, creds: &CredentialStore) -> Result<Self, CliError> {
        let session = creds.get(SESSION_ACCOUNT)?.ok_or_else(|| {
            CliError::Auth("no portal session stored — run `pmac auth login`".into())
        })?;
        let mut portal = Portal::new(cfg)?;
        portal.seed_bundle(&portal.base.clone(), session.expose());
        portal.sync = Some(SessionSync {
            creds: CredentialStore::new(creds.service()),
            last: RefCell::new(session.expose().to_string()),
        });
        Ok(portal)
    }

    /// Seed the identity device-trust cookies so a login can skip MFA.
    pub fn seed_device(&self, bundle: &str) {
        let identity = self.identity.clone();
        self.seed_bundle(&identity, bundle);
    }

    // ---- URL / cookie helpers ---------------------------------------------

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else if path.starts_with('/') {
            format!("{}{}", self.base.trim_end_matches('/'), path)
        } else {
            format!("{}/{}", self.base.trim_end_matches('/'), path)
        }
    }

    fn as_url(s: &str) -> reqwest::Url {
        s.parse().expect("url validated when the client was built")
    }

    /// The OAuth authorize URL. No `state` is sent: the identity server does
    /// not require one here, and omitting it means the client needs nothing
    /// from a prior mypennymac round trip to start a login.
    fn authorize_url(&self) -> String {
        format!(
            "{}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}%2Fcallback",
            self.identity.trim_end_matches('/'),
            self.client_id,
            urlencode(self.base.trim_end_matches('/')),
        )
    }

    fn cookie(&self, base: &str, name: &str) -> Option<String> {
        let header = self.jar.cookies(&Self::as_url(base))?;
        let raw = header.to_str().ok()?;
        raw.split("; ")
            .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
            .map(str::to_string)
    }

    /// The live mypennymac session cookies, serialized for the keychain.
    pub fn session_bundle(&self) -> Option<Secret> {
        let base = self.base.clone();
        let pairs: Vec<String> = SESSION_COOKIES
            .iter()
            .filter_map(|n| self.cookie(&base, n).map(|v| format!("{n}={v}")))
            .collect();
        // `PMST` alone is what authenticates; the rest ride along.
        pairs
            .iter()
            .any(|p| p.starts_with("PMST="))
            .then(|| Secret::new(pairs.join("; ")))
    }

    /// The identity device-trust cookies, serialized for the keychain.
    pub fn device_bundle(&self) -> Option<Secret> {
        let header = self.jar.cookies(&Self::as_url(&self.identity))?;
        let raw = header.to_str().ok()?;
        let pairs: Vec<String> = raw
            .split("; ")
            .filter(|kv| {
                let name = kv.split('=').next().unwrap_or("");
                name.starts_with("ft") || name.contains("tfa") || name == "ids_source"
            })
            .map(str::to_string)
            .collect();
        pairs
            .iter()
            .any(|p| p.contains("tfa"))
            .then(|| Secret::new(pairs.join("; ")))
    }

    fn seed_bundle(&self, base: &str, bundle: &str) {
        let url = Self::as_url(base);
        for pair in bundle.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            self.jar
                .add_cookie_str(&format!("{pair}; Path=/; Secure"), &url);
        }
    }

    /// Persist the session bundle if the portal rotated any of it. Best-effort.
    fn sync_session(&self) {
        let Some(sync) = &self.sync else { return };
        let Some(current) = self.session_bundle() else {
            return;
        };
        let current = current.expose().to_string();
        if *sync.last.borrow() == current {
            return;
        }
        if sync
            .creds
            .set(SESSION_ACCOUNT, &Secret::new(current.clone()))
            .is_ok()
        {
            *sync.last.borrow_mut() = current;
        }
    }

    // ---- Authentication ----------------------------------------------------

    /// Start a login: fetch the sign-in page, then post the credentials.
    ///
    /// Returns [`LoginOutcome::Authenticated`] when the device is trusted and
    /// the OAuth redirect completed to a session cookie, or
    /// [`LoginOutcome::TwoFactorRequired`] when the portal wants an SMS/email
    /// code (see [`Portal::send_code`] / [`Portal::verify_code`]).
    pub fn login(&self, username: &str, password: &Secret) -> Result<LoginOutcome, CliError> {
        let signin =
            self.http.get(self.authorize_url()).send().map_err(|e| {
                CliError::Upstream(format!("fetching the sign-in page failed: {e}"))
            })?;
        let signin_html = signin
            .text()
            .map_err(|e| CliError::Upstream(format!("reading the sign-in page: {e}")))?;
        let token = csrf_token(&signin_html).ok_or_else(|| {
            CliError::Upstream(
                "could not find the login form token — the portal may have changed".into(),
            )
        })?;

        let form = [
            ("authenticity_token", token.as_str()),
            ("user[user_name]", username),
            ("user[password]", &encode_password(password.expose())),
            ("user[remember_device]", "true"),
        ];
        let resp = self
            .http
            .post(format!(
                "{}/users/validate",
                self.identity.trim_end_matches('/')
            ))
            .header("Origin", self.identity.trim_end_matches('/'))
            .form(&form)
            .send()
            .map_err(|e| CliError::Upstream(format!("login request failed: {e}")))?;

        let final_url = resp.url().clone();
        let status = resp.status();
        let body = resp.text().unwrap_or_default();

        if self.session_bundle().is_some() {
            return Ok(LoginOutcome::Authenticated);
        }
        if final_url.path().contains("/users/mfa") || body.contains("id=\"tfaSms\"") {
            return Ok(LoginOutcome::TwoFactorRequired);
        }
        // Back on the sign-in page with the password field means the
        // credentials were rejected — a 200 that a status check would miss.
        if body.contains("id=\"password\"") || final_url.path().contains("/sign_in") {
            return Err(CliError::Auth(
                "invalid username or password — check `pmac config show`, then re-run `pmac auth login`".into(),
            ));
        }
        Err(CliError::Upstream(format!(
            "unexpected login response (HTTP {}, {})",
            status.as_u16(),
            final_url
        )))
    }

    /// Ask the portal to (re)send a second-factor code.
    pub fn send_code(&self, channel: CodeChannel) -> Result<(), CliError> {
        let body = json!({ "mfa_type": channel.mfa_type(), "resend": false });
        let resp = self
            .http
            .post(format!(
                "{}/users/mfa/request_verification",
                self.identity.trim_end_matches('/')
            ))
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", self.identity.trim_end_matches('/'))
            .json(&body)
            .send()
            .map_err(|e| {
                CliError::Upstream(format!("requesting a verification code failed: {e}"))
            })?;
        if !resp.status().is_success() {
            return Err(CliError::Upstream(format!(
                "requesting a verification code returned HTTP {}",
                resp.status().as_u16()
            )));
        }
        Ok(())
    }

    /// Submit a second-factor code, then complete the OAuth handoff to a
    /// mypennymac session.
    pub fn verify_code(&self, code: &str, channel: CodeChannel) -> Result<(), CliError> {
        let body = json!({
            "mfa_type": channel.mfa_type(),
            "remember_me": true,
            "verification_token": code,
        });
        let resp = self
            .http
            .post(format!(
                "{}/users/mfa/verify",
                self.identity.trim_end_matches('/')
            ))
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", self.identity.trim_end_matches('/'))
            .json(&body)
            .send()
            .map_err(|e| CliError::Upstream(format!("verifying the code failed: {e}")))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            return Err(CliError::Auth(format!(
                "verification failed (HTTP {}){}",
                status.as_u16(),
                body_hint(&text)
            )));
        }
        // The identity session is now authenticated; re-run authorize so the
        // client picks up the code and mypennymac sets its session cookie.
        self.complete_oauth()
    }

    /// Run the authorize URL once more on an already-authenticated identity
    /// session, following the redirect to `/callback` that sets `PMST`.
    fn complete_oauth(&self) -> Result<(), CliError> {
        self.http
            .get(self.authorize_url())
            .send()
            .map_err(|e| CliError::Upstream(format!("completing the OAuth handoff failed: {e}")))?;
        if self.session_bundle().is_none() {
            return Err(CliError::Upstream(
                "login succeeded but the portal issued no session cookie".into(),
            ));
        }
        Ok(())
    }

    /// The parked identity session (device + session cookies) for a login that
    /// is waiting on a code, so a later `--code` run can resume it.
    pub fn pending_bundle(&self) -> Option<Secret> {
        let header = self.jar.cookies(&Self::as_url(&self.identity))?;
        let raw = header.to_str().ok()?;
        (!raw.is_empty()).then(|| Secret::new(raw.to_string()))
    }

    /// Seed a parked identity session back in to resume a code challenge.
    pub fn resume_pending(&self, bundle: &str) {
        let identity = self.identity.clone();
        self.seed_bundle(&identity, bundle);
    }

    // ---- Reads -------------------------------------------------------------

    /// Exchange the session cookie for a bearer token + loan number, caching
    /// the result for the rest of the process.
    fn ensure_token(&self) -> Result<(), CliError> {
        if self.token.borrow().is_some() {
            return Ok(());
        }
        let resp = self
            .http
            .get(self.url("/api/account/request_token"))
            .header("Accept", "*/*")
            .send()
            .map_err(|e| CliError::Upstream(format!("request_token failed: {e}")))?;
        let text = self.handle(resp, "/api/account/request_token")?;
        let v = parse_json(&text, "/api/account/request_token")?;
        if v.get("is_logged_in").and_then(Value::as_bool) != Some(true) {
            return Err(CliError::Auth(
                "portal session expired — run `pmac auth login`".into(),
            ));
        }
        let bearer = v
            .get("user_access")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| CliError::Upstream("request_token returned no access token".into()))?;
        let loan_id = v
            .get("loan_numbers")
            .and_then(scalar_string)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| CliError::Upstream("request_token returned no loan number".into()))?;
        *self.token.borrow_mut() = Some(TokenCtx {
            bearer: Secret::new(bearer.to_string()),
            loan_id,
        });
        self.sync_session();
        Ok(())
    }

    /// The loan number this session is scoped to.
    pub fn loan_id(&self) -> Result<String, CliError> {
        self.ensure_token()?;
        Ok(self
            .token
            .borrow()
            .as_ref()
            .expect("token ensured")
            .loan_id
            .clone())
    }

    /// The full `request_token` payload (identity/profile bootstrap).
    pub fn account(&self) -> Result<Value, CliError> {
        let resp = self
            .http
            .get(self.url("/api/account/request_token"))
            .header("Accept", "*/*")
            .send()
            .map_err(|e| CliError::Upstream(format!("request_token failed: {e}")))?;
        let text = self.handle(resp, "/api/account/request_token")?;
        parse_json(&text, "/api/account/request_token")
    }

    /// GET a portal API path with the bearer token attached.
    pub fn get_json(&self, path: &str) -> Result<Value, CliError> {
        self.ensure_token()?;
        let bearer = self.bearer();
        let resp = self
            .http
            .get(self.url(path))
            .header("Accept", "*/*")
            .bearer_auth(bearer.expose())
            .send()
            .map_err(|e| CliError::Upstream(format!("GET {path} failed: {e}")))?;
        let text = self.handle(resp, path)?;
        self.sync_session();
        parse_json(&text, path)
    }

    /// POST a JSON body to a portal API path with the bearer token attached.
    ///
    /// Read-only by construction: every caller in this crate posts to a query
    /// endpoint (the portal uses POST for reads whose filters are JSON bodies).
    /// The `pmac api` guard refuses POSTs to the [`crate::writes`] catalog.
    pub fn post_json(&self, path: &str, body: &Value) -> Result<Value, CliError> {
        self.ensure_token()?;
        let bearer = self.bearer();
        let resp = self
            .http
            .post(self.url(path))
            .header("Accept", "*/*")
            .header("Origin", self.base.trim_end_matches('/'))
            .bearer_auth(bearer.expose())
            .json(body)
            .send()
            .map_err(|e| CliError::Upstream(format!("POST {path} failed: {e}")))?;
        let text = self.handle(resp, path)?;
        self.sync_session();
        parse_json(&text, path)
    }

    /// A read scoped to this session's loan: `POST <path> {"loanId": <id>}`.
    pub fn loan_post(&self, path: &str) -> Result<Value, CliError> {
        let loan = self.loan_id()?;
        self.post_json(path, &json!({ "loanId": loan }))
    }

    fn bearer(&self) -> Secret {
        Secret::new(
            self.token
                .borrow()
                .as_ref()
                .expect("token ensured")
                .bearer
                .expose()
                .to_string(),
        )
    }

    /// Map a response onto the family exit codes, treating a login redirect or
    /// an HTML body where JSON was due as session expiry (exit 3).
    fn handle(&self, resp: reqwest::blocking::Response, path: &str) -> Result<String, CliError> {
        let status = resp.status();
        let final_url = resp.url().clone();
        if matches!(status.as_u16(), 401 | 403) {
            return Err(CliError::Auth(format!(
                "portal returned {} for {path} — run `pmac auth login`",
                status.as_u16()
            )));
        }
        if status.as_u16() == 404 {
            return Err(CliError::NotFound(format!("{path} (HTTP 404)")));
        }
        // A redirect to the identity host is the portal bouncing an expired
        // session to login. It never answers 401 for that.
        if final_url
            .host_str()
            .is_some_and(|h| h.contains("identity."))
        {
            return Err(CliError::Auth(
                "portal session expired — run `pmac auth login`".into(),
            ));
        }
        let text = resp
            .text()
            .map_err(|e| CliError::Upstream(format!("reading response body: {e}")))?;
        if !status.is_success() {
            return Err(CliError::Upstream(format!(
                "portal HTTP {} for {path}{}",
                status.as_u16(),
                body_hint(&text)
            )));
        }
        Ok(text)
    }
}

/// Log in and cache the resulting session in the keychain.
///
/// Shared by the explicit `auth login` and the automatic recovery in
/// [`crate::commands::Ctx::read`], so both prove the session works (via
/// `request_token`) before storing it, and both refresh the device-trust
/// cookies so the next unattended re-auth can still skip MFA.
///
/// Requires a device that has already passed MFA — the stored device cookies
/// are what let this run without a fresh code. If the portal demands a code,
/// that is surfaced as an auth error pointing back at interactive `auth login`.
pub fn establish_session(
    cfg: &Config,
    creds: &CredentialStore,
    username: &str,
    password: &Secret,
) -> Result<(), CliError> {
    let portal = Portal::new(cfg)?;
    if let Some(device) = creds.get(DEVICE_ACCOUNT)? {
        portal.seed_device(device.expose());
    }
    match portal.login(username, password)? {
        LoginOutcome::Authenticated => {}
        LoginOutcome::TwoFactorRequired => {
            return Err(CliError::Auth(
                "portal wants a verification code — run `pmac auth login` interactively".into(),
            ))
        }
    }
    // Prove the session reads before storing it.
    portal.loan_id()?;
    let session = portal.session_bundle().ok_or_else(|| {
        CliError::Upstream("login succeeded but the portal issued no session cookie".into())
    })?;
    creds.set(SESSION_ACCOUNT, &session)?;
    if let Some(device) = portal.device_bundle() {
        creds.set(DEVICE_ACCOUNT, &device)?;
    }
    Ok(())
}

/// The site's own password transform, read out of its login bundle:
/// `btoa(codes.join(","))` where `codes` are the characters' code units. The
/// server rejects a raw password, so this must match exactly.
fn encode_password(password: &str) -> String {
    let codes = password
        .chars()
        .map(|c| (c as u32).to_string())
        .collect::<Vec<_>>()
        .join(",");
    base64_encode(codes.as_bytes())
}

/// Pull the Rails CSRF token out of the sign-in page's `<meta>` tag.
fn csrf_token(html: &str) -> Option<String> {
    let anchor = html.find("name=\"csrf-token\"")?;
    let rest = &html[anchor..];
    let c = rest.find("content=\"")? + "content=\"".len();
    let end = rest[c..].find('"')?;
    Some(rest[c..c + end].to_string())
}

/// Read a JSON scalar (string or number) as a string; `null`/objects → None.
fn scalar_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse a response body as JSON, reporting an HTML body as session expiry.
fn parse_json(text: &str, path: &str) -> Result<Value, CliError> {
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(text).map_err(|_| {
        if text.trim_start().starts_with('<') {
            CliError::Auth(
                "portal returned HTML instead of JSON (session expired) — run `pmac auth login`"
                    .into(),
            )
        } else {
            CliError::Upstream(format!(
                "portal returned non-JSON for {path} (first bytes: {:?})",
                text.chars().take(60).collect::<String>()
            ))
        }
    })
}

/// Pull a short human hint out of an error body for error messages.
fn body_hint(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        for ptr in ["/message", "/error", "/errors/0", "/ResponseStatus/Message"] {
            if let Some(m) = v.pointer(ptr).and_then(Value::as_str) {
                if !m.is_empty() {
                    return format!(" — {m}");
                }
            }
        }
    }
    format!(" — {}", trimmed.chars().take(120).collect::<String>())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Standard base64 (with padding). Hand-rolled to avoid a dependency for the
/// one place the login needs it.
fn base64_encode(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[((n >> 18) & 63) as usize] as char);
        out.push(A[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            A[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn portal() -> Portal {
        Portal::new(&Config::default()).unwrap()
    }

    #[test]
    fn urls_join_base_and_path() {
        let p = portal();
        assert_eq!(
            p.url("/api/loan/loans"),
            "https://mypennymac.pennymac.com/api/loan/loans"
        );
        assert_eq!(p.url("https://other/y"), "https://other/y");
    }

    #[test]
    fn authorize_url_targets_the_identity_host_and_callback() {
        let p = portal();
        let u = p.authorize_url();
        assert!(
            u.starts_with("https://identity.pennymac.com/oauth/authorize"),
            "{u}"
        );
        assert!(u.contains("client_id="), "{u}");
        assert!(
            u.contains("redirect_uri=https%3A%2F%2Fmypennymac.pennymac.com%2Fcallback"),
            "{u}"
        );
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn password_uses_the_sites_charcode_scheme() {
        // "AB" -> char codes 65,66 -> "65,66" -> base64.
        assert_eq!(encode_password("AB"), base64_encode(b"65,66"));
        // The scheme is base64 of the comma-joined codes, never the raw string.
        assert_ne!(encode_password("AB"), base64_encode(b"AB"));
    }

    #[test]
    fn csrf_token_is_scraped_from_the_meta_tag() {
        let html = r#"<meta name="csrf-param" content="authenticity_token" />
                      <meta name="csrf-token" content="abc123-TOKEN_x" />"#;
        assert_eq!(csrf_token(html).as_deref(), Some("abc123-TOKEN_x"));
        assert_eq!(csrf_token("<html>no token</html>"), None);
    }

    #[test]
    fn session_bundle_needs_the_authenticating_cookie() {
        let p = portal();
        p.seed_bundle(&p.base.clone(), "AWSALB=x; AWSALBCORS=y");
        assert!(p.session_bundle().is_none());
        p.seed_bundle(&p.base.clone(), "PMST=abc");
        let bundle = p.session_bundle().expect("bundle present");
        assert!(bundle.expose().contains("PMST=abc"), "{}", bundle.expose());
    }

    #[test]
    fn device_bundle_captures_the_trust_cookies_only() {
        let p = portal();
        p.seed_device("ft5=a; _secure_pennymacusa_com_tfa=b; ids_source=web; _ga=noise");
        let bundle = p.device_bundle().expect("device bundle present");
        let raw = bundle.expose();
        assert!(raw.contains("_secure_pennymacusa_com_tfa=b"), "{raw}");
        assert!(raw.contains("ft5=a"), "{raw}");
        assert!(!raw.contains("_ga="), "analytics cookie leaked: {raw}");
    }

    #[test]
    fn parse_json_reads_html_as_expiry() {
        assert!(matches!(
            parse_json("<!doctype html>", "/x").unwrap_err(),
            CliError::Auth(_)
        ));
        assert_eq!(parse_json("  ", "/x").unwrap(), Value::Null);
        assert!(matches!(
            parse_json("kaboom", "/x").unwrap_err(),
            CliError::Upstream(_)
        ));
    }

    #[test]
    fn body_hint_prefers_a_message_field() {
        assert_eq!(body_hint(""), "");
        assert_eq!(body_hint(r#"{"message":"nope"}"#), " — nope");
        assert_eq!(body_hint("plain").as_str(), " — plain");
    }
}
