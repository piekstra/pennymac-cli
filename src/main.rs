//! `pmac` — piekstra-family CLI for the Pennymac mortgage servicing portal
//! (`mypennymac.pennymac.com`).
//!
//! Conforms to piekstra-cli/1. Read-only: every command observes, none mutate.
//! The portal's write surface — payments, autopay, saved bank accounts, profile
//! edits — is catalogued in `src/writes.rs` and printed by `pmac writes`,
//! deliberately unimplemented.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use pk_cli_auth::{AuthStatus, LoginArgs, LogoutArgs, SetCredentialArgs};
use pk_cli_config::ConfigStore;
use pk_cli_core::info::{AuthInfo, CliInfo};
use pk_cli_core::{output, CliError, CommonArgs};
use pk_cli_secrets::CredentialStore;
use pk_cli_selfupdate::{SelfUpdateArgs, Updater};

use pmac::client::{CodeChannel, LoginOutcome, Portal};
use pmac::commands::{
    api, autopay, documents, escrow, messages, methods, payments, profile, summary, transactions,
    writes, Ctx,
};
use pmac::config::{
    self, Config, DEVICE_ACCOUNT, KEYCHAIN_ACCOUNT, PENDING_ACCOUNT, SESSION_ACCOUNT,
};

const BIN: &str = "pmac";
const REPO: &str = "piekstra/pennymac-cli";

/// Pennymac mortgage servicing portal from the command line — balance, escrow,
/// statements, transactions, and documents. Read-only. Unofficial.
#[derive(Parser, Debug)]
#[command(name = BIN, version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Portal login, session status, and credential management.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// Non-secret settings.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Loan overview: amount due, due date, balances, rate, property.
    Summary,
    /// Same overview as `summary` — the balance-first entry point.
    Balance,
    /// Escrow balance, monthly components, and tax/insurance disbursements.
    Escrow,
    /// Automatic-payment (ACH) status: enrollment, next draft, cutoff.
    Autopay,
    /// The loan ledger: payments, disbursements, interest.
    #[command(subcommand)]
    Transactions(transactions::Cmd),
    /// Posted mortgage payments.
    #[command(subcommand)]
    Payments(payments::Cmd),
    /// Saved payment methods, masked as the portal masks them.
    #[command(subcommand)]
    Methods(methods::Cmd),
    /// Statements, escrow analyses, and tax forms the portal published.
    #[command(subcommand, visible_alias = "statements")]
    Documents(documents::Cmd),
    /// Message-center notices from the servicer.
    #[command(subcommand)]
    Messages(messages::Cmd),
    /// The account holder on file.
    Profile,
    /// Portal write endpoints this CLI deliberately does not implement.
    Writes,
    /// Raw portal API passthrough (read-only guarded).
    Api(api::ApiArgs),
    /// Update to the latest release from GitHub.
    SelfUpdate(SelfUpdateArgs),
    /// Print a shell completion script.
    Completions { shell: Shell },
    /// Machine-readable capability discovery (cli-info/v1).
    Info,
}

#[derive(Subcommand, Debug)]
enum AuthCmd {
    /// Log in to the portal and cache the session.
    Login(PortalLoginArgs),
    /// Report credential and session state (auth-status/v1).
    Status(StatusArgs),
    /// Clear the cached session; --forget also removes the stored password.
    Logout(LogoutArgs),
    /// Raw keychain write for rotation / headless setup.
    SetCredential(SetCredentialArgs),
}

#[derive(clap::Args, Debug)]
struct PortalLoginArgs {
    #[command(flatten)]
    base: LoginArgs,
    /// Second-factor delivery channel for the first login on a new device.
    #[arg(long, value_enum, default_value = "sms")]
    channel: CodeChannel,
    /// Verification code, to resume a login that requested one.
    #[arg(long, value_name = "CODE")]
    code: Option<String>,
}

#[derive(clap::Args, Debug)]
struct StatusArgs {
    /// Prove the stored session against the portal (a live request_token call),
    /// not just that a session is present. Exits 3 if it's been expired.
    #[arg(long)]
    verify: bool,
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Show the effective configuration.
    Show,
    /// Set a config key (base_url, identity_url, client_id, username, auto_login).
    Set { key: String, value: String },
    /// Remove a config key.
    Unset { key: String },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        std::process::exit(output::fail(&e, cli.common.json));
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    let store = ConfigStore::new(BIN);
    let creds = CredentialStore::for_binary(BIN);
    let cfg: Config = store.load()?;
    let ctx = Ctx {
        common: &cli.common,
        cfg: &cfg,
        creds: &creds,
    };

    match &cli.command {
        Command::Auth(cmd) => auth(cli, cmd, &store, &creds, &cfg),
        Command::Config(cmd) => config_cmd(cli, cmd, &store),
        Command::Summary | Command::Balance => summary::run(&ctx),
        Command::Escrow => escrow::run(&ctx),
        Command::Autopay => autopay::run(&ctx),
        Command::Transactions(cmd) => transactions::run(&ctx, cmd),
        Command::Payments(cmd) => payments::run(&ctx, cmd),
        Command::Methods(cmd) => methods::run(&ctx, cmd),
        Command::Documents(cmd) => documents::run(&ctx, cmd),
        Command::Messages(cmd) => messages::run(&ctx, cmd),
        Command::Profile => profile::run(&ctx),
        Command::Writes => writes::run(&ctx),
        Command::Api(args) => api::run(&ctx, args),
        Command::SelfUpdate(args) => Updater {
            repo: REPO.into(),
            binary: BIN.into(),
            target: env!("BUILD_TARGET").into(),
            current: env!("CARGO_PKG_VERSION").into(),
        }
        .run(args, cli.common.json, cli.common.quiet),
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), BIN, &mut std::io::stdout());
            Ok(())
        }
        Command::Info => {
            let info = CliInfo::new(
                BIN,
                env!("CARGO_PKG_VERSION"),
                &format!("https://github.com/{REPO}"),
                AuthInfo {
                    required: true,
                    method: "password".into(),
                    login_hint: Some(format!("{BIN} auth login")),
                },
                &[
                    "summary",
                    "balance",
                    "escrow",
                    "autopay",
                    "transactions",
                    "payments",
                    "methods",
                    "documents",
                    "messages",
                    "profile",
                    "writes",
                    "api",
                ],
            );
            output::json(&serde_json::to_value(&info).unwrap());
            Ok(())
        }
    }
}

fn auth(
    cli: &Cli,
    cmd: &AuthCmd,
    store: &ConfigStore,
    creds: &CredentialStore,
    cfg: &Config,
) -> Result<(), CliError> {
    match cmd {
        AuthCmd::Login(args) => login(cli, args, creds, cfg),
        AuthCmd::Status(args) => status(cli, args, creds, cfg),
        AuthCmd::Logout(args) => {
            creds.delete(SESSION_ACCOUNT)?;
            creds.delete(PENDING_ACCOUNT)?;
            if args.forget {
                creds.delete(KEYCHAIN_ACCOUNT)?;
                creds.delete(DEVICE_ACCOUNT)?;
                store.clear()?;
                if !cli.common.quiet {
                    eprintln!("session cleared; password and device trust removed");
                }
            } else if !cli.common.quiet {
                eprintln!("session cleared (password kept; use --forget to remove it)");
            }
            Ok(())
        }
        AuthCmd::SetCredential(args) => {
            if creds.get(KEYCHAIN_ACCOUNT)?.is_some() && !args.overwrite {
                return Err(CliError::Usage(
                    "a password is already stored; pass --overwrite to replace it".into(),
                ));
            }
            let secret = args.source.read(None)?;
            creds.set(KEYCHAIN_ACCOUNT, &secret)?;
            if !cli.common.quiet {
                eprintln!("password stored in the OS keychain ({})", creds.service());
            }
            Ok(())
        }
    }
}

/// Full portal login, handling the OAuth + SMS-MFA flow.
fn login(
    cli: &Cli,
    args: &PortalLoginArgs,
    creds: &CredentialStore,
    cfg: &Config,
) -> Result<(), CliError> {
    // A code on the command line belongs to the login that requested it, so
    // resume that parked session rather than starting a fresh one.
    if let Some(code) = &args.code {
        let pending = creds.get(PENDING_ACCOUNT)?.ok_or_else(|| {
            CliError::Usage("no login is waiting for a code — run `pmac auth login` first".into())
        })?;
        let portal = Portal::new(cfg)?;
        portal.resume_pending(pending.expose());
        let result = portal.verify_code(code.trim(), args.channel);
        creds.delete(PENDING_ACCOUNT)?; // the parked session is spent either way
        result?;
        return finish_login(cli, &portal, creds);
    }

    let username = cfg.username().ok_or_else(|| {
        CliError::Usage(format!(
            "no portal username configured — run `{BIN} config set username <you>`"
        ))
    })?;

    // Password from the keychain, falling back to the standard ingestion flags.
    let password = match creds.get(KEYCHAIN_ACCOUNT)? {
        Some(p) if !args.base.overwrite => p,
        _ => {
            let prompt = if args.base.non_interactive {
                None
            } else {
                Some("Portal password")
            };
            let secret = args.base.source.read(prompt)?;
            creds.set(KEYCHAIN_ACCOUNT, &secret)?;
            secret
        }
    };

    let portal = Portal::new(cfg)?;
    if let Some(device) = creds.get(DEVICE_ACCOUNT)? {
        portal.seed_device(device.expose());
    }

    match portal.login(&username, &password)? {
        LoginOutcome::Authenticated => finish_login(cli, &portal, creds),
        LoginOutcome::TwoFactorRequired => {
            portal.send_code(args.channel)?;
            // Park the identity session that owns this code so `--code` resumes it.
            if let Some(pending) = portal.pending_bundle() {
                creds.set(PENDING_ACCOUNT, &pending)?;
            }
            if !cli.common.interactive() {
                return Err(CliError::Auth(
                    "a verification code was sent — re-run with `pmac auth login --code <CODE>`"
                        .into(),
                ));
            }
            eprintln!("A verification code was sent to your phone/email on file.");
            let code = prompt_line("Verification code: ")?;
            let result = portal.verify_code(code.trim(), args.channel);
            creds.delete(PENDING_ACCOUNT)?;
            result?;
            finish_login(cli, &portal, creds)
        }
    }
}

/// Store the freshly minted session (and refreshed device trust) after a login.
fn finish_login(cli: &Cli, portal: &Portal, creds: &CredentialStore) -> Result<(), CliError> {
    // Prove the session reads before storing it.
    portal.loan_id()?;
    let session = portal.session_bundle().ok_or_else(|| {
        CliError::Upstream("login succeeded but the portal issued no session cookie".into())
    })?;
    creds.set(SESSION_ACCOUNT, &session)?;
    if let Some(device) = portal.device_bundle() {
        creds.set(DEVICE_ACCOUNT, &device)?;
    }
    if !cli.common.quiet {
        eprintln!("session cached in the OS keychain ({})", creds.service());
    }
    Ok(())
}

fn status(
    cli: &Cli,
    args: &StatusArgs,
    creds: &CredentialStore,
    cfg: &Config,
) -> Result<(), CliError> {
    let has_password = creds.get(KEYCHAIN_ACCOUNT)?.is_some();
    let has_session = creds.get(SESSION_ACCOUNT)?.is_some();

    // A stored session is necessary but not sufficient — the portal expires
    // sessions server-side. Only --verify proves it against the portal.
    let (authenticated, note) = if !args.verify || !has_session {
        (has_session, None)
    } else {
        match Portal::from_cached_session(cfg, creds).and_then(|p| p.loan_id()) {
            Ok(_) => (true, Some("session verified against the portal")),
            Err(CliError::Auth(_)) => (
                false,
                Some("stored session rejected — run `pmac auth login`"),
            ),
            Err(e) => return Err(e), // a network failure says nothing about the credential
        }
    };

    let mut st = AuthStatus::new(true, authenticated, pk_cli_auth::AuthMethod::Password);
    st.username = cfg.username();
    st.credential_in_keychain = Some(has_password);
    st.emit(cli.common.json);
    if let Some(note) = note {
        if !cli.common.quiet {
            eprintln!("{note}");
        }
    }
    // The DTO already went to stdout; exit directly rather than returning an
    // error that would print a second JSON document after it.
    if args.verify && !authenticated {
        std::process::exit(CliError::Auth(String::new()).exit_code());
    }
    Ok(())
}

fn config_cmd(cli: &Cli, cmd: &ConfigCmd, store: &ConfigStore) -> Result<(), CliError> {
    match cmd {
        ConfigCmd::Path => {
            println!("{}", store.path()?.display());
            Ok(())
        }
        ConfigCmd::Show => {
            let cfg: Config = store.load()?;
            let v = serde_json::to_value(&cfg).unwrap_or_default();
            if cli.common.json {
                output::json(&v);
            } else {
                output::render(&v);
            }
            Ok(())
        }
        ConfigCmd::Set { key, value } => {
            let mut cfg: Config = store.load()?;
            match key.as_str() {
                "base_url" => cfg.base_url = Some(value.clone()),
                "identity_url" => cfg.identity_url = Some(value.clone()),
                "client_id" => cfg.client_id = Some(value.clone()),
                "username" => cfg.username = Some(value.clone()),
                "auto_login" => cfg.auto_login = Some(parse_bool(value)?),
                other => return Err(unknown_key(other)),
            }
            store.save(&cfg)
        }
        ConfigCmd::Unset { key } => {
            let mut cfg: Config = store.load()?;
            match key.as_str() {
                "base_url" => cfg.base_url = None,
                "identity_url" => cfg.identity_url = None,
                "client_id" => cfg.client_id = None,
                "username" => cfg.username = None,
                "auto_login" => cfg.auto_login = None,
                other => return Err(unknown_key(other)),
            }
            store.save(&cfg)
        }
    }
}

/// Read a single line from stdin for an interactive prompt. The verification
/// code is single-use and lives for minutes — not a secret worth hiding as it
/// is typed.
fn prompt_line(prompt: &str) -> Result<String, CliError> {
    use std::io::Write;
    eprint!("{prompt}");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| CliError::Other(format!("reading input: {e}")))?;
    Ok(line.trim().to_string())
}

fn parse_bool(v: &str) -> Result<bool, CliError> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        other => Err(CliError::Usage(format!(
            "expected a boolean (true/false), got `{other}`"
        ))),
    }
}

fn unknown_key(key: &str) -> CliError {
    CliError::Usage(format!(
        "unknown config key `{key}` (known: {})",
        config::KNOWN_KEYS.join(", ")
    ))
}
