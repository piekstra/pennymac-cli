//! `pmac payments list` — just the borrower payments from the ledger.

use clap::Subcommand;
use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::json;

use super::{emit, items, note_empty, paginate, table_view, Ctx, LOAN_ACTIVITY};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List posted payments (payment-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    let Cmd::List(range) = cmd;
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
