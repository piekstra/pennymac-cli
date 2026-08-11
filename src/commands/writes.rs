//! `pmac writes` — the portal's mutating endpoints this read-only CLI
//! deliberately does not implement. Prints the [`crate::writes`] catalog.

use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{emit, table_view, Ctx};
use crate::writes::CAPABILITIES;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let items: Vec<Value> = CAPABILITIES
        .iter()
        .map(|c| {
            json!({
                "method": c.method,
                "path": c.path,
                "category": c.category.as_str(),
                "description": c.description,
            })
        })
        .collect();

    emit(
        ctx,
        "write-capability-list",
        json!({ "items": items }),
        |v| {
            if !ctx.common.quiet {
                eprintln!(
                "pmac is read-only. These portal endpoints are catalogued but NOT implemented:\n"
            );
            }
            output::table(&table_view(
                &super::items(v, "items"),
                &["category", "method", "path", "description"],
            ));
        },
    );
    Ok(())
}
