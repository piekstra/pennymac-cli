//! `pmac messages list` — the message center: notices the servicer posted.

use clap::Subcommand;
use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::json;

use super::{emit, items, note_empty, paginate, table_view, Ctx, MESSAGES};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List message-center entries (message-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    let Cmd::List(range) = cmd;
    range.validate()?;
    let raw = ctx.read(|c| c.get_json(MESSAGES))?;
    let rows = paginate(parse::messages(&raw), "received", range);
    if rows.is_empty() {
        note_empty(ctx, "messages");
    }
    emit(ctx, "message-list", json!({ "items": rows }), |v| {
        output::table(&table_view(
            &items(v, "items"),
            &["received", "read", "id", "body"],
        ));
    });
    Ok(())
}
