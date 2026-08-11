//! `pmac escrow` — escrow balance, the monthly escrow components, the last
//! analysis, and upcoming tax/insurance disbursements.

use pk_cli_core::{output, CliError};
use serde_json::Value;

use super::{emit, Ctx, LOANS};
use crate::parse;

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let loans = ctx.read(|c| c.loan_post(LOANS))?;
    let payload = parse::escrow(&loans);
    emit(ctx, "escrow-detail", payload, |v| {
        // Render the scalar fields, then the disbursement rows as a table.
        let mut scalars = v.clone();
        let taxes = scalars
            .as_object_mut()
            .and_then(|m| m.remove("tax_disbursements"));
        output::kv(&scalars, 0);
        if let Some(Value::Array(rows)) = taxes {
            if !rows.is_empty() {
                println!("\nTax disbursements:");
                output::table(&super::table_view(
                    &rows,
                    &["authority", "next_disbursement_date", "projected_amount"],
                ));
            }
        }
    });
    Ok(())
}
