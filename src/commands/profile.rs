//! `pmac profile` — the account holder on file (from the session bootstrap).

use pk_cli_core::{output, CliError};

use super::{emit, Ctx};
use crate::parse;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let account = ctx.read(|c| c.account())?;
    let payload = parse::profile(&account);
    emit(ctx, "profile", payload, |v| output::kv(v, 0));
    Ok(())
}
