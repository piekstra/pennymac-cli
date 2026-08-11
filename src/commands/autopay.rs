//! `pmac autopay` — automatic-payment (ACH) enrollment status, the next
//! scheduled draft, monthly amount, and the daily cutoff.

use pk_cli_core::{output, CliError};

use super::{emit, Ctx, PAYMENT_INFO};
use crate::parse;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let info = ctx.read(|c| c.loan_post(PAYMENT_INFO))?;
    let payload = parse::autopay(&info);
    emit(ctx, "autopay-status", payload, |v| output::kv(v, 0));
    Ok(())
}
