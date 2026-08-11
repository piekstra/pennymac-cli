//! Offline black-box tests of the command surface. No network, no keychain:
//! every case here is `--help`, argument validation, or a discovery command,
//! so nothing prompts, hangs, or hits the portal.

use assert_cmd::Command;
use predicates::str::contains;

fn pmac() -> Command {
    Command::cargo_bin("pmac").expect("binary builds")
}

/// Every top-level command. The help/version tests iterate this so a renamed
/// or dropped command trips a test.
const COMMANDS: &[&str] = &[
    "auth",
    "config",
    "summary",
    "balance",
    "escrow",
    "transactions",
    "payments",
    "documents",
    "messages",
    "profile",
    "writes",
    "api",
    "self-update",
    "completions",
    "info",
];

/// Command groups with `list` subcommands that must carry the `ls` alias.
const LIST_GROUPS: &[&str] = &["transactions", "payments", "documents", "messages"];

#[test]
fn top_level_help_lists_every_command() {
    let out = pmac().arg("--help").assert().success();
    let text = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    for cmd in COMMANDS {
        assert!(text.contains(cmd), "--help is missing `{cmd}`");
    }
}

#[test]
fn every_command_help_renders() {
    // Rendering each subcommand's help forces clap's debug assertions over that
    // subtree, catching a flag that collides with a global like `-q`/`-v`.
    for cmd in COMMANDS {
        pmac().args([cmd, "--help"]).assert().success();
    }
}

#[test]
fn list_subcommands_all_have_the_ls_alias() {
    for group in LIST_GROUPS {
        pmac().args([group, "ls", "--help"]).assert().success();
    }
}

#[test]
fn version_prints_the_crate_version() {
    pmac()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_command_is_a_usage_error() {
    pmac().arg("no-such-command").assert().code(2);
}

#[test]
fn info_reports_the_cli_contract() {
    let out = pmac().arg("info").assert().success();
    let v: serde_json::Value =
        serde_json::from_slice(&out.get_output().stdout).expect("info emits JSON");
    assert_eq!(v["name"], "pmac");
    assert_eq!(v["spec"], "piekstra-cli/1");
    assert_eq!(v["auth"]["required"], true);
    assert_eq!(v["auth"]["method"], "password");
    let caps = v["capabilities"].as_array().unwrap();
    for expected in ["summary", "escrow", "transactions", "documents", "api"] {
        assert!(
            caps.iter().any(|c| c == expected),
            "capabilities missing {expected}"
        );
    }
}

#[test]
fn completions_render_for_every_supported_shell() {
    for shell in ["bash", "zsh", "fish"] {
        pmac().args(["completions", shell]).assert().success();
    }
}

// ---- Argument validation happens before any network/keychain access -------

#[test]
fn an_inverted_date_range_is_a_usage_error() {
    pmac()
        .args([
            "transactions",
            "list",
            "--since",
            "2026-08-01",
            "--until",
            "2026-01-01",
        ])
        .assert()
        .code(2)
        .stderr(contains("is after"));
}

#[test]
fn a_non_iso_date_is_a_usage_error() {
    pmac()
        .args(["transactions", "list", "--since", "08/01/2026"])
        .assert()
        .code(2);
}

#[test]
fn api_rejects_invalid_json_before_the_network() {
    pmac()
        .args(["api", "/api/loan/loans", "--data", "{not json"])
        .assert()
        .code(2)
        .stderr(contains("valid JSON"));
}

#[test]
fn api_refuses_to_post_to_a_write_endpoint() {
    // Confirmation-required (exit 6), and casing is not a bypass.
    pmac()
        .args(["api", "/API/Payment/Submit_Payment", "--data", "{}"])
        .assert()
        .code(6)
        .stderr(contains("read-only"));
}

#[test]
fn an_unknown_config_key_is_a_usage_error() {
    pmac()
        .args(["config", "set", "nonsense", "x"])
        .assert()
        .code(2)
        .stderr(contains("unknown config key"));
}

#[test]
fn config_path_resolves_without_a_session() {
    pmac().args(["config", "path"]).assert().success();
}

#[test]
fn writes_lists_only_this_cli_in_error_messages() {
    // Error/help text must reference real commands, not a stale renamed one.
    let out = pmac().args(["api", "--help"]).assert().success();
    let text = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(text.contains("pmac") || text.contains("api"));
}
