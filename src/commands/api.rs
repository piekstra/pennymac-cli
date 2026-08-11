//! `pmac api` — raw passthrough to any portal API endpoint.
//!
//! The escape hatch for surfaces the typed commands don't cover, and the
//! quickest way to check whether the portal's responses have drifted. The
//! bearer token and session cookie are attached automatically.
//!
//! POST is supported because several of the portal's *reads* are POSTs (their
//! filters are JSON bodies) — but a POST to any endpoint in [`crate::writes`]
//! is refused, so the escape hatch cannot move money while this CLI advertises
//! itself as read-only.

use clap::Args;
use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::Ctx;
use crate::writes;

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// Path under the portal host, e.g. `/api/loan/loans`.
    pub path: String,

    /// JSON body to POST. Without it the request is a GET. Use the literal
    /// `{}` to POST the loan-scoped default `{"loanId": <your loan>}`.
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,

    /// Inject `{"loanId": <your loan>}` into the POST body (merged with
    /// `--data` if both are given). Most loan reads want this.
    #[arg(long)]
    pub loan: bool,
}

pub fn run(ctx: &Ctx, args: &ApiArgs) -> Result<(), CliError> {
    // Validate everything before opening a session or touching the network.
    let body = match &args.data {
        Some(raw) => Some(
            serde_json::from_str::<Value>(raw)
                .map_err(|e| CliError::Usage(format!("--data must be valid JSON: {e}")))?,
        ),
        None => None,
    };
    let is_post = body.is_some() || args.loan;

    if is_post && writes::is_write(&args.path) {
        return Err(CliError::ConfirmationRequired(format!(
            "{} is a write endpoint and this CLI is read-only — see `pmac writes`",
            args.path
        )));
    }

    let payload = ctx.read(|c| {
        if is_post {
            let mut obj = match body.clone() {
                Some(Value::Object(m)) => m,
                Some(other) => return c.post_json(&args.path, &other),
                None => serde_json::Map::new(),
            };
            if args.loan {
                obj.insert("loanId".into(), json!(c.loan_id()?));
            }
            c.post_json(&args.path, &Value::Object(obj))
        } else {
            c.get_json(&args.path)
        }
    })?;

    output::json(&payload);
    Ok(())
}
