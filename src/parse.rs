//! Pure mappers from raw portal JSON to the CLI's DTO shapes.
//!
//! Kept free of I/O so the integration tests can run them against captured
//! fixtures: a portal field rename then fails a test instead of silently
//! emptying a column. Every function is total — unfamiliar or missing shapes
//! yield empty/absent fields, never a panic.

use serde_json::{json, Map, Value};

use crate::dates::iso_date;

/// Money as the family DTO shape: a string decimal, never a float.
fn money(v: Option<&Value>) -> Option<Value> {
    let amount = match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }?;
    Some(json!({ "amount": format!("{amount:.2}"), "currency": "USD" }))
}

fn num(v: &Value, key: &str) -> Option<Value> {
    money(v.get(key))
}

fn f64_at(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn date_at(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).and_then(iso_date)
}

fn str_at(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// Insert `key => value` when `value` is `Some`, preserving build order.
fn put(map: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
    if let Some(v) = value {
        map.insert(key.to_string(), v.into());
    }
}

/// The single loan summary object out of `/api/loan/loans`.
fn summary_obj(loans: &Value) -> Option<&Value> {
    loans.get("loanSummary")?.as_array()?.first()
}

/// Assemble the property address from its parts, dropping blanks.
fn property_address(s: &Value) -> Option<String> {
    let parts = [
        str_at(s, "propertyAddressLine1"),
        str_at(s, "propertyAddressLine2"),
        str_at(s, "propertyLocality"),
        str_at(s, "propertyRegion"),
        str_at(s, "propertyPostalCode"),
    ];
    let joined: Vec<String> = parts.into_iter().flatten().collect();
    (!joined.is_empty()).then(|| joined.join(", "))
}

/// The overview: amount due, due date, balances, rate, property.
pub fn loan_summary(loans: &Value) -> Value {
    let Some(s) = summary_obj(loans) else {
        return json!({});
    };
    let balances = s.get("balanceSummary").unwrap_or(&Value::Null);
    // The loan number comes from the session (request_token), not this
    // payload; the command prepends it. Lead with the money the dashboard leads
    // with — amount due and its date.
    let mut m = Map::new();
    put(
        &mut m,
        "amount_due",
        num(s, "currentTotalMonthlyPaymentAmount"),
    );
    put(&mut m, "due_date", date_at(s, "nextPaymentDueDate"));
    put(
        &mut m,
        "principal_balance",
        num(balances, "unpaidPrincipalBalanceAmount")
            .or_else(|| num(s, "unpaidPrincipalBalanceAmount")),
    );
    put(
        &mut m,
        "escrow_balance",
        num(balances, "escrowBalanceAmount").or_else(|| num(s, "escrowBalanceAmount")),
    );
    if let Some(rate) = f64_at(s, "currentInterestRate").filter(|r| *r > 0.0) {
        m.insert(
            "interest_rate".into(),
            json!(format!("{:.3}%", rate * 100.0)),
        );
    }
    put(
        &mut m,
        "monthly_principal_interest",
        num(s, "currentMonthlyPaymentAmount"),
    );
    put(
        &mut m,
        "monthly_total",
        num(s, "currentTotalMonthlyPaymentAmount"),
    );
    put(
        &mut m,
        "last_payment_date",
        date_at(s, "lastPaymentReceivedDate"),
    );
    if let Some(count) = s.get("delinquentPaymentCount").and_then(Value::as_i64) {
        m.insert("delinquent_payments".into(), json!(count));
    }
    put(&mut m, "maturity_date", date_at(s, "loanMaturityDate"));
    put(&mut m, "original_amount", num(s, "originalLoanAmount"));
    if let Some(term) = s.get("loanAmortizationTerm").and_then(Value::as_i64) {
        m.insert("term_months".into(), json!(term));
    }
    if let Some(rem) = s.get("currentAmortizationTerm").and_then(Value::as_i64) {
        m.insert("remaining_term_months".into(), json!(rem));
    }
    put(
        &mut m,
        "property_address",
        property_address(s).map(Value::String),
    );
    put(
        &mut m,
        "investor",
        str_at(s, "investorName").map(Value::String),
    );
    put(
        &mut m,
        "ytd_principal_paid",
        num(s, "yearToDatePrincipalPaidAmount"),
    );
    put(
        &mut m,
        "ytd_interest_paid",
        num(s, "yearToDateTotalInterestPaidAmount"),
    );
    put(&mut m, "escrow_enrolled", s.get("escrowFlag").cloned());
    Value::Object(m)
}

/// Escrow detail: balance, the monthly components, and tax disbursements.
pub fn escrow(loans: &Value) -> Value {
    let Some(s) = summary_obj(loans) else {
        return json!({});
    };
    let balances = s.get("balanceSummary").unwrap_or(&Value::Null);
    let mut m = Map::new();
    put(
        &mut m,
        "escrow_balance",
        num(balances, "escrowBalanceAmount").or_else(|| num(s, "escrowBalanceAmount")),
    );
    put(
        &mut m,
        "monthly_mortgage_insurance",
        num(s, "monthlyMortgageInsuranceAmount"),
    );
    put(
        &mut m,
        "monthly_hazard_insurance",
        num(s, "monthlyHazardInsuranceAmount"),
    );
    put(
        &mut m,
        "monthly_county_tax",
        num(s, "monthlyCountyTaxAmount"),
    );
    put(&mut m, "monthly_city_tax", num(s, "monthlyCityTaxAmount"));
    put(
        &mut m,
        "last_analysis_date",
        date_at(s, "lastEscrowAnalysisDate"),
    );
    put(
        &mut m,
        "last_analysis_over_short",
        num(s, "lastEscrowAnalysisOverShortAmount"),
    );
    put(&mut m, "ytd_escrow_paid", num(s, "yearToDateEscrowPaid"));
    put(
        &mut m,
        "next_mortgage_insurance_due",
        date_at(s, "mortgageInsuranceNextPremiumDueDate"),
    );

    let taxes = s
        .get("escrowTax")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|t| {
                    let mut tm = Map::new();
                    put(
                        &mut tm,
                        "authority",
                        str_at(t, "taxAuthorityType").map(Value::String),
                    );
                    put(
                        &mut tm,
                        "next_disbursement_date",
                        date_at(t, "taxNextDisbursementDueDate"),
                    );
                    put(
                        &mut tm,
                        "projected_amount",
                        num(t, "taxProjectedDisbursementAmount"),
                    );
                    Value::Object(tm)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !taxes.is_empty() {
        m.insert("tax_disbursements".into(), Value::Array(taxes));
    }
    Value::Object(m)
}

/// The transaction ledger out of `/api/loan/get_loan_activity`.
fn history(activity: &Value) -> Vec<Value> {
    activity
        .as_array()
        .and_then(|a| a.first())
        .and_then(|first| first.get("history"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn transaction_row(t: &Value) -> Value {
    let mut m = Map::new();
    put(
        &mut m,
        "date",
        date_at(t, "effectiveDate").map(Value::String),
    );
    put(
        &mut m,
        "type",
        str_at(t, "transactionType").map(Value::String),
    );
    put(
        &mut m,
        "description",
        str_at(t, "transactionCodeDesc").map(Value::String),
    );
    put(&mut m, "amount", num(t, "transactionAmount"));
    put(&mut m, "principal", num(t, "principalAmount"));
    put(&mut m, "interest", num(t, "interestAmount"));
    put(&mut m, "escrow", num(t, "escrowAmount"));
    put(&mut m, "principal_balance", num(t, "principalBalance"));
    if let Some(id) = t.get("transactionId").and_then(Value::as_i64) {
        m.insert("id".into(), json!(id));
    }
    Value::Object(m)
}

/// All ledger transactions, newest first as the portal returns them.
pub fn transactions(activity: &Value) -> Vec<Value> {
    history(activity).iter().map(transaction_row).collect()
}

/// Just the borrower payments from the ledger.
pub fn payments(activity: &Value) -> Vec<Value> {
    history(activity)
        .iter()
        .filter(|t| {
            str_at(t, "transactionType")
                .map(|s| s.eq_ignore_ascii_case("Payment"))
                .unwrap_or(false)
        })
        .map(transaction_row)
        .collect()
}

/// Documents/statements out of `/api/documents/get_docs`.
pub fn documents(docs: &Value) -> Vec<Value> {
    docs.get("whiteListedDoc")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|d| {
                    let mut m = Map::new();
                    // documents/v1 canonicalizes `id` as a string (GUID- and
                    // precision-safe, one shape across providers). PennyMac's
                    // docId is numeric; stringify it here so the DTO conforms.
                    if let Some(id) = d.get("docId").and_then(Value::as_i64) {
                        m.insert("id".into(), json!(id.to_string()));
                    }
                    put(
                        &mut m,
                        "name",
                        str_at(d, "docTypeFriendly").map(Value::String),
                    );
                    put(&mut m, "category", str_at(d, "docType").map(Value::String));
                    put(
                        &mut m,
                        "date",
                        date_at(d, "effectiveUTC").map(Value::String),
                    );
                    put(
                        &mut m,
                        "uploaded",
                        date_at(d, "uploaded").map(Value::String),
                    );
                    put(&mut m, "file", str_at(d, "fileName").map(Value::String));
                    Value::Object(m)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Message-center entries out of `/api/messages/get_messages`.
pub fn messages(msgs: &Value) -> Vec<Value> {
    msgs.as_array()
        .map(|rows| {
            rows.iter()
                .map(|m0| {
                    let mut m = Map::new();
                    if let Some(id) = m0.get("messageId").and_then(Value::as_i64) {
                        m.insert("id".into(), json!(id));
                    }
                    put(
                        &mut m,
                        "received",
                        date_at(m0, "receivedDateTimeUtc").map(Value::String),
                    );
                    if let Some(read) = m0.get("messageRead").and_then(Value::as_bool) {
                        m.insert("read".into(), json!(read));
                    }
                    put(&mut m, "body", str_at(m0, "body").map(Value::String));
                    Value::Object(m)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The account holder / profile out of `/api/account/request_token`.
pub fn profile(account: &Value) -> Value {
    let up = account.get("user_profile").unwrap_or(&Value::Null);
    let mut m = Map::new();
    put(
        &mut m,
        "username",
        str_at(account, "user_name").map(Value::String),
    );
    let name = [str_at(up, "first_name"), str_at(up, "last_name")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if !name.is_empty() {
        m.insert("name".into(), json!(name));
    }
    put(
        &mut m,
        "email",
        str_at(up, "verified_email").map(Value::String),
    );
    put(
        &mut m,
        "phone",
        str_at(up, "verified_phone_number").map(Value::String),
    );
    put(
        &mut m,
        "customer_id",
        str_at(up, "customer_id").map(Value::String),
    );
    put(
        &mut m,
        "loan_numbers",
        str_at(account, "loan_numbers").map(Value::String),
    );
    Value::Object(m)
}

/// Autopay / ACH status out of `/api/payment/get_payment_info`.
///
/// Reads **only** the non-sensitive status flags and the current scheduled
/// payment's amounts and dates. It deliberately never descends into any
/// `bankAccount` sub-object — those carry raw routing numbers and an encoded
/// account number that this read-only CLI must not surface. See the negative
/// assertion in `tests/fixture_shapes.rs`.
pub fn autopay(info: &Value) -> Value {
    let mut m = Map::new();
    if let Some(active) = info.get("isACHActive").and_then(Value::as_bool) {
        m.insert("enrolled".into(), json!(active));
    }
    if let Some(pending) = info.get("isACHPending").and_then(Value::as_bool) {
        m.insert("enrollment_pending".into(), json!(pending));
    }
    // The scheduled payment: amount, extras, and the next dates.
    let pay = info.get("payment").unwrap_or(&Value::Null);
    put(&mut m, "monthly_payment", num(pay, "monthlyPaymentAmount"));
    if let Some(extra) = f64_at(pay, "additionalPrincipalAmount").filter(|v| *v > 0.0) {
        m.insert(
            "additional_principal".into(),
            json!({ "amount": format!("{extra:.2}"), "currency": "USD" }),
        );
    }
    if let Some(extra) = f64_at(pay, "additionalEscrowAmount").filter(|v| *v > 0.0) {
        m.insert(
            "additional_escrow".into(),
            json!({ "amount": format!("{extra:.2}"), "currency": "USD" }),
        );
    }
    put(&mut m, "next_due_date", date_at(pay, "dueDate"));
    put(
        &mut m,
        "next_draft_date",
        date_at(info, "achProjectFisrtDraftDate"),
    );
    // Which account autopay draws from — masked account + routing as returned.
    let bank = pay.get("bankAccount").unwrap_or(&Value::Null);
    put(&mut m, "draft_account", str_at(bank, "accountNumber"));
    put(
        &mut m,
        "draft_routing",
        str_at(bank, "routingTransitNumber"),
    );
    put(&mut m, "cutoff_time", str_at(info, "cutOffTimePSTEnd"));
    if let Some(can) = info.get("canMakePayment").and_then(Value::as_bool) {
        m.insert("can_make_payment".into(), json!(can));
    }
    if let Some(n) = info.get("numberofPendingPayments").and_then(Value::as_i64) {
        m.insert("pending_payments".into(), json!(n));
    }
    Value::Object(m)
}

/// Saved payment methods out of `/api/payment/get_bank_accounts`.
///
/// This is the user reading their own accounts on file, so it surfaces the full
/// useful view: the label, the account holder, the routing number, the account
/// number as the portal returns it (already masked to the last four), whether
/// it's validated, and when it was last used. It skips only fields that carry
/// no reader value — the encoded/duplicate account blobs, internal ids, and the
/// opaque numeric `bankAccountType` (which is not a checking/savings flag).
/// Removed accounts are dropped.
pub fn payment_methods(accounts: &Value) -> Vec<Value> {
    accounts
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|a| !a.get("isRemoved").and_then(Value::as_bool).unwrap_or(false))
                .map(|a| {
                    let mut m = Map::new();
                    put(&mut m, "nickname", str_at(a, "accountNickName"));
                    put(&mut m, "account_holder", str_at(a, "accountName"));
                    put(&mut m, "routing", str_at(a, "routingTransitNumber"));
                    // As the portal returns it — masked to the last four.
                    put(&mut m, "account", str_at(a, "accountNumber"));
                    if let Some(v) = a.get("isValidated").and_then(Value::as_bool) {
                        m.insert("validated".into(), json!(v));
                    }
                    put(&mut m, "last_used", date_at(a, "lastUsed"));
                    Value::Object(m)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A money DTO from a raw float, when it's present and non-zero.
fn money_if_positive(v: &Value, key: &str) -> Option<Value> {
    f64_at(v, key)
        .filter(|n| *n > 0.0)
        .map(|n| json!({ "amount": format!("{n:.2}"), "currency": "USD" }))
}

/// Scheduled / in-flight payments out of `pendingPayments[]` in
/// `/api/payment/get_payment_info` — the drafts the portal will make (or is
/// processing) but that haven't posted to the ledger yet. Distinct from
/// `payments list`, which reads the posted ledger.
pub fn pending_payments(info: &Value) -> Vec<Value> {
    info.get("pendingPayments")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|p| {
                    let mut m = Map::new();
                    put(&mut m, "effective_date", date_at(p, "effectiveDate"));
                    put(&mut m, "due_date", date_at(p, "dueDate"));
                    put(&mut m, "amount", num(p, "totalDraftAmount"));
                    put(
                        &mut m,
                        "additional_principal",
                        money_if_positive(p, "additionalPrincipalAmount"),
                    );
                    put(
                        &mut m,
                        "additional_escrow",
                        money_if_positive(p, "additionalEscrowAmount"),
                    );
                    put(&mut m, "confirmation", str_at(p, "displayedConfirmationId"));
                    if let Some(c) = p.get("canCancel").and_then(Value::as_bool) {
                        m.insert("cancelable".into(), json!(c));
                    }
                    // The funding account, as the portal returns it (masked).
                    let bank = p.get("bankAccount").unwrap_or(&Value::Null);
                    put(&mut m, "draft_account", str_at(bank, "accountNumber"));
                    put(&mut m, "scheduled_on", date_at(p, "created"));
                    Value::Object(m)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The deep loan view out of `/api/loan/loans`: the characteristics behind the
/// `summary` numbers — servicer, investor, rate type, terms, key dates, the
/// property, mortgage insurance, and the payment-history pattern.
pub fn loan_detail(loans: &Value) -> Value {
    let Some(s) = summary_obj(loans) else {
        return json!({});
    };
    let svc = s.get("organizationServicerSummary").unwrap_or(&Value::Null);
    let mut m = Map::new();

    put(&mut m, "servicer", str_at(svc, "organizationName"));
    put(
        &mut m,
        "servicing_type",
        str_at(svc, "servicingContractType"),
    );
    put(&mut m, "investor", str_at(s, "investorName"));
    put(&mut m, "investor_code", str_at(s, "investorCode"));

    if let Some(rate) = f64_at(s, "currentInterestRate").filter(|r| *r > 0.0) {
        m.insert(
            "interest_rate".into(),
            json!(format!("{:.3}%", rate * 100.0)),
        );
    }
    if let Some(rate) = f64_at(s, "initialInterestRate").filter(|r| *r > 0.0) {
        m.insert(
            "original_rate".into(),
            json!(format!("{:.3}%", rate * 100.0)),
        );
    }
    if let Some(arm) = s.get("armFlag").and_then(Value::as_bool) {
        m.insert("adjustable_rate".into(), json!(arm));
    }
    if let Some(io) = s.get("interestOnlyFlag").and_then(Value::as_bool) {
        m.insert("interest_only".into(), json!(io));
    }

    put(&mut m, "original_amount", num(s, "originalLoanAmount"));
    put(
        &mut m,
        "monthly_principal_interest",
        num(s, "currentMonthlyPaymentAmount"),
    );
    put(
        &mut m,
        "monthly_total",
        num(s, "currentTotalMonthlyPaymentAmount"),
    );
    if let Some(t) = s.get("loanAmortizationTerm").and_then(Value::as_i64) {
        m.insert("term_months".into(), json!(t));
    }
    if let Some(t) = s.get("currentAmortizationTerm").and_then(Value::as_i64) {
        m.insert("remaining_term_months".into(), json!(t));
    }

    put(
        &mut m,
        "first_payment_date",
        date_at(s, "scheduledFirstPaymentDate"),
    );
    put(&mut m, "next_due_date", date_at(s, "nextPaymentDueDate"));
    put(&mut m, "interest_paid_to", date_at(s, "interestPaidToDate"));
    put(&mut m, "maturity_date", date_at(s, "loanMaturityDate"));

    if let Some(count) = s.get("delinquentPaymentCount").and_then(Value::as_i64) {
        m.insert("delinquent_payments".into(), json!(count));
    }
    // A right-to-left run of month codes ('0' = paid on time); handy raw.
    put(
        &mut m,
        "payment_history",
        str_at(s, "paymentHistoryPattern"),
    );

    put(
        &mut m,
        "monthly_mortgage_insurance",
        num(s, "monthlyMortgageInsuranceAmount"),
    );
    put(
        &mut m,
        "mi_next_due",
        date_at(s, "mortgageInsuranceNextPremiumDueDate"),
    );

    put(
        &mut m,
        "property_address",
        property_address(s).map(Value::String),
    );
    put(&mut m, "county", str_at(s, "propertyCountyCode"));
    put(&mut m, "occupancy", str_at(s, "propertyOccupancyTypeDesc"));
    if let Some(units) = s
        .get("propertyFinancedNumberOfUnits")
        .and_then(Value::as_i64)
    {
        m.insert("units".into(), json!(units));
    }
    put(
        &mut m,
        "year_built",
        str_at(s, "propertyStructureBuiltYear"),
    );
    put(
        &mut m,
        "appraised_value",
        num(s, "propertyAppraisedValueAmount"),
    );
    put(
        &mut m,
        "original_value",
        num(s, "originalPropertyValueAmount"),
    );

    put(&mut m, "escrow_enrolled", s.get("escrowFlag").cloned());
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappers_tolerate_unrecognized_shapes() {
        for junk in [json!({}), json!([]), json!(null), json!("nope")] {
            assert_eq!(loan_summary(&junk), json!({}));
            assert_eq!(escrow(&junk), json!({}));
            assert!(transactions(&junk).is_empty());
            assert!(payments(&junk).is_empty());
            assert!(documents(&junk).is_empty());
            assert!(messages(&junk).is_empty());
        }
    }

    #[test]
    fn money_is_a_string_decimal_never_a_float() {
        let v = json!({ "loanSummary": [{ "currentTotalMonthlyPaymentAmount": 1234.56 }] });
        let dto = loan_summary(&v);
        assert_eq!(dto["amount_due"]["amount"], json!("1234.56"));
        assert_eq!(dto["amount_due"]["currency"], json!("USD"));
    }

    #[test]
    fn interest_rate_renders_as_a_percent() {
        let v = json!({ "loanSummary": [{ "currentInterestRate": 0.0599 }] });
        assert_eq!(loan_summary(&v)["interest_rate"], json!("5.990%"));
        // A zero rate is omitted rather than shown as 0.000%.
        let z = json!({ "loanSummary": [{ "currentInterestRate": 0.0 }] });
        assert!(loan_summary(&z).get("interest_rate").is_none());
    }

    #[test]
    fn payments_filter_the_ledger_to_borrower_payments() {
        let activity = json!([{
            "history": [
                { "transactionType": "Payment", "effectiveDate": "2026-01-05T00:00:00", "transactionAmount": 1234.56 },
                { "transactionType": "Escrow Disbursement", "effectiveDate": "2026-01-05T00:00:00", "transactionAmount": -141.90 },
            ]
        }]);
        assert_eq!(transactions(&activity).len(), 2);
        let pays = payments(&activity);
        assert_eq!(pays.len(), 1);
        assert_eq!(pays[0]["type"], json!("Payment"));
    }
}
