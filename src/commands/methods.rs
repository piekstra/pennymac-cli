//! `pmac methods list` — saved payment methods: label, account holder, routing
//! number, and the account number as the portal returns it (masked to the last
//! four), with validated and last-used.

use clap::Subcommand;
use pk_cli_core::{output, CliError};
use serde_json::json;

use super::{emit, items, note_empty, table_view, Ctx, BANK_ACCOUNTS};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List saved payment methods, masked (payment-method-list/v1).
    #[command(visible_alias = "ls")]
    List,
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    let Cmd::List = cmd;
    let accounts = ctx.read(|c| c.loan_post(BANK_ACCOUNTS))?;
    let rows = parse::payment_methods(&accounts);
    if rows.is_empty() {
        note_empty(ctx, "payment methods");
    }
    emit(ctx, "payment-method-list", json!({ "items": rows }), |v| {
        output::table(&table_view(
            &items(v, "items"),
            &[
                "nickname",
                "account_holder",
                "routing",
                "account",
                "validated",
                "last_used",
            ],
        ));
    });
    Ok(())
}
