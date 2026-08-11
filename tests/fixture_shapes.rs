//! Contract tests over the scrubbed captures in `tests/fixtures/`.
//!
//! Two jobs: prove the parsers read the fields the table views actually render
//! (so a portal rename fails loudly), and prove the fixtures leak no real
//! identifier (so nobody publishes one by accident).

use std::fs;
use std::path::PathBuf;

use pmac::parse;
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {path:?}: {e}"))
}

fn is_iso_date(v: &Value) -> bool {
    v.as_str()
        .map(|s| s.len() == 10 && s.matches('-').count() == 2)
        .unwrap_or(false)
}

fn is_money(v: &Value) -> bool {
    v.get("amount").and_then(Value::as_str).is_some()
        && v.get("currency").and_then(Value::as_str) == Some("USD")
}

#[test]
fn loan_summary_reads_the_fields_the_overview_renders() {
    let dto = parse::loan_summary(&fixture("loans.json"));
    assert!(is_money(&dto["amount_due"]), "amount_due: {dto:#}");
    assert!(is_money(&dto["principal_balance"]));
    assert!(is_money(&dto["escrow_balance"]));
    assert!(is_iso_date(&dto["due_date"]));
    assert!(is_iso_date(&dto["maturity_date"]));
    assert_eq!(dto["interest_rate"], serde_json::json!("5.000%"));
    assert_eq!(dto["delinquent_payments"], serde_json::json!(0));
    assert!(dto["property_address"]
        .as_str()
        .unwrap()
        .contains("Sample City"));
}

#[test]
fn escrow_reads_components_and_tax_disbursements() {
    let dto = parse::escrow(&fixture("loans.json"));
    assert!(is_money(&dto["escrow_balance"]));
    assert!(is_money(&dto["monthly_mortgage_insurance"]));
    assert!(is_money(&dto["monthly_county_tax"]));
    assert!(is_iso_date(&dto["last_analysis_date"]));
    let taxes = dto["tax_disbursements"].as_array().expect("tax array");
    assert_eq!(taxes.len(), 1);
    assert_eq!(taxes[0]["authority"], serde_json::json!("County"));
    assert!(is_iso_date(&taxes[0]["next_disbursement_date"]));
    assert!(is_money(&taxes[0]["projected_amount"]));
}

#[test]
fn transactions_and_payments_split_the_ledger() {
    let activity = fixture("loan_activity.json");
    let txns = parse::transactions(&activity);
    assert_eq!(txns.len(), 2, "both history rows are transactions");
    for t in &txns {
        assert!(is_iso_date(&t["date"]), "each txn has an ISO date: {t:#}");
        assert!(is_money(&t["amount"]));
    }
    let pays = parse::payments(&activity);
    assert_eq!(pays.len(), 1, "only the Payment row is a payment");
    assert_eq!(pays[0]["type"], serde_json::json!("Payment"));
    assert!(is_money(&pays[0]["principal"]));
}

#[test]
fn documents_read_name_category_and_dates() {
    let docs = parse::documents(&fixture("get_docs.json"));
    assert_eq!(docs.len(), 3);
    let names: Vec<&str> = docs.iter().filter_map(|d| d["name"].as_str()).collect();
    assert!(names.contains(&"Monthly Statement"));
    assert!(names.contains(&"Escrow Analysis"));
    for d in &docs {
        assert!(is_iso_date(&d["date"]), "doc has an ISO date: {d:#}");
        assert!(d["id"].is_i64(), "doc id is numeric: {d:#}");
    }
}

#[test]
fn messages_read_received_read_flag_and_body() {
    let msgs = parse::messages(&fixture("get_messages.json"));
    assert_eq!(msgs.len(), 2);
    assert!(is_iso_date(&msgs[0]["received"]));
    assert_eq!(msgs[0]["read"], serde_json::json!(false));
    assert_eq!(msgs[1]["read"], serde_json::json!(true));
    assert!(msgs[0]["body"].as_str().unwrap().contains("Statement"));
}

#[test]
fn profile_reads_the_account_holder() {
    let dto = parse::profile(&fixture("request_token.json"));
    assert_eq!(dto["username"], serde_json::json!("sampleuser"));
    assert_eq!(dto["name"], serde_json::json!("Sample Owner"));
    assert_eq!(dto["loan_numbers"], serde_json::json!("1000000000"));
}

/// A portal redesign should empty a table, not crash the CLI: every parser
/// must survive shapes it doesn't recognize.
#[test]
fn parsers_tolerate_unrecognized_shapes() {
    for junk in [
        serde_json::json!({}),
        serde_json::json!([]),
        serde_json::json!(null),
        serde_json::json!("<html>maintenance</html>"),
    ] {
        assert_eq!(parse::loan_summary(&junk), serde_json::json!({}));
        assert_eq!(parse::escrow(&junk), serde_json::json!({}));
        assert!(parse::transactions(&junk).is_empty());
        assert!(parse::payments(&junk).is_empty());
        assert!(parse::documents(&junk).is_empty());
        assert!(parse::messages(&junk).is_empty());
        assert_eq!(parse::profile(&junk), serde_json::json!({}));
    }
}

// ---- Scrub enforcement --------------------------------------------------

/// Every 8-4-4-4-12 hex UUID in a fixture, found without a regex dependency.
fn uuids(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let seg = |start: usize, len: usize| -> bool {
        start + len <= bytes.len()
            && bytes[start..start + len]
                .iter()
                .all(|b| b.is_ascii_hexdigit())
    };
    let mut out = Vec::new();
    let hyphens = [8, 13, 18, 23];
    for i in 0..bytes.len() {
        if i + 36 <= bytes.len()
            && hyphens.iter().all(|&h| bytes[i + h] == b'-')
            && seg(i, 8)
            && seg(i + 9, 4)
            && seg(i + 14, 4)
            && seg(i + 19, 4)
            && seg(i + 24, 12)
        {
            out.push(s[i..i + 36].to_string());
        }
    }
    out
}

#[test]
fn uuid_scanner_finds_what_it_should() {
    let found =
        uuids("a 00000000-0000-0000-0000-000000000000 b 12345678-9abc-def0-1234-567890abcdef");
    assert_eq!(found.len(), 2);
    assert!(uuids("not-a-uuid 1234").is_empty());
}

/// The positive rule: fixtures may carry only *obvious dummy* identifiers.
///
/// A denylist of patterns (never real values — that would itself be the leak)
/// catches the shapes a real capture leaks; the UUID rule catches everything
/// else, since any UUID that isn't the all-zero dummy is a live identifier.
#[test]
fn fixtures_carry_no_real_identifiers() {
    // Patterns, not real values: real-name fragments, live-credential markers,
    // cookie/header names, and address tells that a genuine capture would carry.
    const BANNED: &[&str] = &[
        "piekstra",
        "caleb",
        "@gmail",
        "@pennymac",
        "authorization",
        "bearer ",
        "set-cookie",
        "pmst=",
        "_secure_pennymacusa",
        "enzi",
        "saint lucie",
        "port saint",
        "freddie",
    ];
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("reading fixtures dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).unwrap();
        let lower = raw.to_ascii_lowercase();
        for needle in BANNED {
            assert!(
                !lower.contains(needle),
                "{}: contains banned identifier pattern {needle:?}",
                path.display()
            );
        }
        for uuid in uuids(&raw) {
            assert!(
                uuid.chars().all(|c| c == '0' || c == '-'),
                "{}: contains a live UUID {uuid} — replace it with the all-zero dummy",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 6, "expected the full fixture set, saw {checked}");
}
