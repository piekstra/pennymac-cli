//! `pmac documents list` (alias `statements`) — statements, escrow analyses,
//! 1098 tax forms, and other documents the portal has published — and
//! `pmac documents download <id>`, which streams one of them to a file.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use pk_cli_core::{output, CliError};
use pk_cli_utility::RangeArgs;
use serde_json::{json, Value};

use super::{emit, items, note_empty, paginate, table_view, Ctx, DOCUMENTS};
use crate::parse;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List published documents (document-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
    /// Download a published document to a file (document-download/v1).
    #[command(visible_alias = "get")]
    Download(DownloadArgs),
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// The document id (the `id` column of `documents list`).
    id: i64,
    /// Where to write it: a file path, an existing directory, or `-` for
    /// stdout. Defaults to the portal's filename in the current directory.
    #[arg(short, long)]
    output: Option<String>,
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    match cmd {
        Cmd::List(range) => list(ctx, range),
        Cmd::Download(args) => download(ctx, args),
    }
}

fn list(ctx: &Ctx, range: &RangeArgs) -> Result<(), CliError> {
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

fn download(ctx: &Ctx, args: &DownloadArgs) -> Result<(), CliError> {
    // Resolve the id against the published list first: it gives the real
    // filename to request and to name the output, and a clean 404 when the id
    // isn't one of the borrower's documents.
    let docs = ctx.read(|c| c.loan_post(DOCUMENTS))?;
    let doc = parse::documents(&docs)
        .into_iter()
        .find(|d| d.get("id").and_then(Value::as_i64) == Some(args.id))
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "document {} not found — see `pmac documents list`",
                args.id
            ))
        })?;
    let file_name = doc
        .get("file")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.pdf", args.id));
    let name = doc
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("document")
        .to_string();

    let bytes = ctx.read(|c| c.download_doc(args.id, &file_name))?;

    // `-o -` makes the file itself the stdout data stream; diagnostics go to
    // stderr so a pipe stays clean.
    if args.output.as_deref() == Some("-") {
        std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| CliError::Upstream(format!("writing document to stdout: {e}")))?;
        if !ctx.common.quiet {
            eprintln!("wrote {} bytes ({name}) to stdout", bytes.len());
        }
        return Ok(());
    }

    let path = resolve_output(args.output.as_deref(), &file_name);
    std::fs::write(&path, &bytes)
        .map_err(|e| CliError::Upstream(format!("writing {}: {e}", path.display())))?;

    let payload = json!({
        "id": args.id,
        "name": name,
        "category": doc.get("category").cloned().unwrap_or(Value::Null),
        "date": doc.get("date").cloned().unwrap_or(Value::Null),
        "file": file_name,
        "path": path.display().to_string(),
        "bytes": bytes.len(),
    });
    emit(ctx, "document-download", payload, |v| {
        println!(
            "Saved {} → {} ({} bytes)",
            v.get("name").and_then(Value::as_str).unwrap_or("document"),
            v.get("path").and_then(Value::as_str).unwrap_or("?"),
            v.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        );
    });
    Ok(())
}

/// A file path as given, a filename joined onto a directory, or the portal's
/// filename in the current directory when nothing was asked for.
fn resolve_output(output: Option<&str>, file_name: &str) -> PathBuf {
    match output {
        None => PathBuf::from(file_name),
        Some(o) => {
            let p = PathBuf::from(o);
            if p.is_dir() {
                p.join(file_name)
            } else {
                p
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_output;
    use std::path::PathBuf;

    #[test]
    fn output_defaults_to_the_portal_filename() {
        assert_eq!(resolve_output(None, "stmt.pdf"), PathBuf::from("stmt.pdf"));
    }

    #[test]
    fn an_explicit_path_is_used_verbatim() {
        assert_eq!(
            resolve_output(Some("/tmp/x.pdf"), "stmt.pdf"),
            PathBuf::from("/tmp/x.pdf")
        );
    }

    #[test]
    fn a_directory_target_gets_the_filename_appended() {
        // The current directory always exists, so it's a stable "is a dir" case.
        assert_eq!(
            resolve_output(Some("."), "stmt.pdf"),
            PathBuf::from("./stmt.pdf")
        );
    }
}
