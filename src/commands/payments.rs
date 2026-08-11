//! `pmac payments list` — posted payments from the ledger.
//! `pmac payments pending` — scheduled/in-flight drafts not yet posted.

use clap::Subcommand;
use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::json;

use super::{emit, items, note_empty, paginate, table_view, Ctx, LOAN_ACTIVITY, PAYMENT_INFO};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List posted payments (payment-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
    /// List scheduled / in-flight payments not yet posted (pending-payment-list/v1).
    Pending,
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    match cmd {
        Cmd::List(range) => list(ctx, range),
        Cmd::Pending => pending(ctx),
    }
}

fn list(ctx: &Ctx, range: &RangeArgs) -> Result<(), CliError> {
    range.validate()?;
    let activity = ctx.read(|c| c.post_json(LOAN_ACTIVITY, &json!({ "loanId": c.loan_id()? })))?;
    let rows = paginate(parse::payments(&activity), "date", range);
    if rows.is_empty() {
        note_empty(ctx, "payments");
    }
    emit(ctx, "payment-list", json!({ "items": rows }), |v| {
        output::table(&table_view(
            &items(v, "items"),
            &[
                "date",
                "description",
                "amount",
                "principal",
                "interest",
                "escrow",
            ],
        ));
    });
    Ok(())
}

fn pending(ctx: &Ctx) -> Result<(), CliError> {
    let info = ctx.read(|c| c.loan_post(PAYMENT_INFO))?;
    let rows = parse::pending_payments(&info);
    if rows.is_empty() {
        note_empty(ctx, "pending payments");
    }
    emit(ctx, "pending-payment-list", json!({ "items": rows }), |v| {
        output::table(&table_view(
            &items(v, "items"),
            &[
                "effective_date",
                "due_date",
                "amount",
                "confirmation",
                "draft_account",
                "cancelable",
            ],
        ));
    });
    Ok(())
}
