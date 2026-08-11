//! `pmac loan` — the deep loan view: servicer, investor, rate type, terms, key
//! dates, the property, mortgage insurance, and the payment-history pattern.
//! The characteristics behind the `summary` numbers.

use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{emit, Ctx, LOANS};
use crate::parse;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let (loan_id, loans) = ctx.read(|c| Ok((c.loan_id()?, c.loan_post(LOANS)?)))?;

    let mut payload = json!({ "loan_number": loan_id });
    if let (Value::Object(head), Value::Object(rest)) = (&mut payload, parse::loan_detail(&loans)) {
        head.extend(rest);
    }

    emit(ctx, "loan-detail", payload, |v| output::kv(v, 0));
    Ok(())
}
