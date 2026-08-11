//! `pmac documents list` (alias `statements`) — statements, escrow analyses,
//! 1098 tax forms, and other documents the portal has published.

use clap::Subcommand;
use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::json;

use super::{emit, items, note_empty, paginate, table_view, Ctx, DOCUMENTS};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List published documents (document-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    let Cmd::List(range) = cmd;
    range.validate()?;
    let docs = ctx.read(|c| c.loan_post(DOCUMENTS))?;
    let rows = paginate(parse::documents(&docs), "date", range);
    if rows.is_empty() {
        note_empty(ctx, "documents");
    }
    emit(ctx, "document-list", json!({ "items": rows }), |v| {
        output::table(&table_view(
            &items(v, "items"),
            &["date", "name", "category", "id"],
        ));
    });
    Ok(())
}
