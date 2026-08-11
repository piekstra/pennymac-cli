//! `pmac transactions list` — the full loan ledger (payments, disbursements,
//! interest), newest first, with `--since` / `--until` / `--limit` filtering.

use clap::Subcommand;
use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::json;

use super::{emit, items, note_empty, paginate, table_view, Ctx, LOAN_ACTIVITY};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List ledger transactions (transaction-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    let Cmd::List(range) = cmd;
    range.validate()?;

    let activity = ctx.read(|c| c.post_json(LOAN_ACTIVITY, &json!({ "loanId": c.loan_id()? })))?;
    let rows = paginate(parse::transactions(&activity), "date", range);
    if rows.is_empty() {
        note_empty(ctx, "transactions");
    }

    emit(ctx, "transaction-list", json!({ "items": rows }), |v| {
        output::table(&table_view(
            &items(v, "items"),
            &["date", "type", "description", "amount", "principal_balance"],
        ));
    });
    Ok(())
}
