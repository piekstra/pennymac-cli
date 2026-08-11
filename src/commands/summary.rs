//! `pmac summary` (alias `balance`) — the loan at a glance: amount due, due
//! date, balances, rate, and property.

use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{emit, Ctx, LOANS};
use crate::parse;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    // Loan number and summary come from one retry unit so an expiry can't
    // render half of it.
    let (loan_id, loans) = ctx.read(|c| Ok((c.loan_id()?, c.loan_post(LOANS)?)))?;

    let mut payload = json!({ "loan_number": loan_id });
    if let (Value::Object(head), Value::Object(rest)) = (&mut payload, parse::loan_summary(&loans))
    {
        head.extend(rest);
    }

    emit(ctx, "loan-summary", payload, |v| output::kv(v, 0));
    Ok(())
}
