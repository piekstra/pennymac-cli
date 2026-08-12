//! `pmac documents` — the published-document surface:
//!
//! - `list` (alias `statements`): statements, escrow analyses, 1098s, …
//! - `download <id>` (alias `get`) or `download --all`: stream document(s) to
//!   files, a directory, or stdout.
//! - `open <id>`: download to a temp file and hand it to the system viewer.

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
    /// Download a document — one by id, or every one with --all
    /// (document-download/v1, document-download-batch/v1).
    #[command(visible_alias = "get")]
    Download(DownloadArgs),
    /// Download a document and open it in the system viewer (document-open/v1).
    Open(OpenArgs),
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// A document id from `documents list`. Omit and pass --all for every one.
    id: Option<i64>,
    /// Download every published document (filter with --since/--until/--limit).
    #[arg(long, conflicts_with = "id")]
    all: bool,
    /// Output target: a file path or `-` for stdout (single id), or a directory
    /// (with --all). Defaults to the portal's filename(s) in the current dir.
    #[arg(short, long)]
    output: Option<String>,
    #[command(flatten)]
    range: RangeArgs,
}

#[derive(Args, Debug)]
pub struct OpenArgs {
    /// A document id from `documents list`.
    id: i64,
}

pub fn run(ctx: &Ctx, cmd: &Cmd) -> Result<(), CliError> {
    match cmd {
        Cmd::List(range) => list(ctx, range),
        Cmd::Download(args) => download(ctx, args),
        Cmd::Open(args) => open(ctx, args.id),
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
    match (args.all, args.id) {
        (true, _) => download_all(ctx, args),
        (false, Some(id)) => download_one(ctx, id, args),
        (false, None) => Err(CliError::Usage(
            "give a document id (see `pmac documents list`) or --all".into(),
        )),
    }
}

fn download_one(ctx: &Ctx, id: i64, args: &DownloadArgs) -> Result<(), CliError> {
    let (doc, bytes) = fetch_doc(ctx, id)?;
    let file_name = file_name_of(&doc, id);
    let name = doc_name(&doc);

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

    emit(
        ctx,
        "document-download",
        saved_dto(&doc, id, &path, bytes.len()),
        |v| {
            println!("{}", saved_line(v));
        },
    );
    Ok(())
}

fn download_all(ctx: &Ctx, args: &DownloadArgs) -> Result<(), CliError> {
    if args.output.as_deref() == Some("-") {
        return Err(CliError::Usage(
            "--all can't stream to stdout; give a directory with -o, or omit it".into(),
        ));
    }
    args.range.validate()?;

    // One session, one token, one reauth boundary for the whole batch: a mid-run
    // expiry re-logs in and replays from the top (nothing is written yet).
    let downloaded = fetch_all(ctx, &args.range)?;

    let dir = args
        .output
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_default();
    if !dir.as_os_str().is_empty() && !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| CliError::Upstream(format!("creating {}: {e}", dir.display())))?;
    }

    let mut written = Vec::with_capacity(downloaded.len());
    let mut bytes_total: u64 = 0;
    for (doc, bytes) in &downloaded {
        let id = doc_id(doc).unwrap_or_default();
        let path = if dir.as_os_str().is_empty() {
            PathBuf::from(file_name_of(doc, id))
        } else {
            dir.join(file_name_of(doc, id))
        };
        std::fs::write(&path, bytes)
            .map_err(|e| CliError::Upstream(format!("writing {}: {e}", path.display())))?;
        bytes_total += bytes.len() as u64;
        written.push(saved_dto(doc, id, &path, bytes.len()));
    }

    if written.is_empty() {
        note_empty(ctx, "documents");
    }
    let where_to = if dir.as_os_str().is_empty() {
        ".".to_string()
    } else {
        dir.display().to_string()
    };
    let payload = json!({
        "count": written.len(),
        "bytes_total": bytes_total,
        "dir": where_to,
        "items": written,
    });
    emit(ctx, "document-download-batch", payload, |v| {
        for it in v
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            println!("{}", saved_line(it));
        }
        println!(
            "{} document(s), {} bytes → {}",
            v.get("count").and_then(Value::as_u64).unwrap_or(0),
            v.get("bytes_total").and_then(Value::as_u64).unwrap_or(0),
            v.get("dir").and_then(Value::as_str).unwrap_or("."),
        );
    });
    Ok(())
}

fn open(ctx: &Ctx, id: i64) -> Result<(), CliError> {
    let (doc, bytes) = fetch_doc(ctx, id)?;
    let path = std::env::temp_dir().join(file_name_of(&doc, id));
    std::fs::write(&path, &bytes)
        .map_err(|e| CliError::Upstream(format!("writing {}: {e}", path.display())))?;

    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&path)
        .spawn()
        .map_err(|e| {
            CliError::Upstream(format!(
                "saved to {} but couldn't launch `{opener}`: {e}",
                path.display()
            ))
        })?;

    let payload = json!({
        // documents/v1: `id` is a string (see parse::documents).
        "id": id.to_string(),
        "name": doc_name(&doc),
        "file": file_name_of(&doc, id),
        "path": path.display().to_string(),
        "opened_with": opener,
    });
    emit(ctx, "document-open", payload, |v| {
        println!(
            "Opened {} → {} (via {})",
            v.get("name").and_then(Value::as_str).unwrap_or("document"),
            v.get("path").and_then(Value::as_str).unwrap_or("?"),
            v.get("opened_with").and_then(Value::as_str).unwrap_or("?"),
        );
    });
    Ok(())
}

// ---- shared helpers -------------------------------------------------------

/// Resolve one id to its metadata + bytes in a single reauth-wrapped session.
fn fetch_doc(ctx: &Ctx, id: i64) -> Result<(Value, Vec<u8>), CliError> {
    ctx.read(|c| {
        let docs = c.loan_post(DOCUMENTS)?;
        let want = id.to_string();
        let doc = parse::documents(&docs)
            .into_iter()
            .find(|d| d.get("id").and_then(Value::as_str) == Some(want.as_str()))
            .ok_or_else(|| {
                CliError::NotFound(format!(
                    "document {id} not found — see `pmac documents list`"
                ))
            })?;
        let bytes = c.download_doc(id, &file_name_of(&doc, id))?;
        Ok((doc, bytes))
    })
}

/// Fetch every document matching the range, as metadata + bytes, in one
/// reauth-wrapped session.
fn fetch_all(ctx: &Ctx, range: &RangeArgs) -> Result<Vec<(Value, Vec<u8>)>, CliError> {
    ctx.read(|c| {
        let docs = c.loan_post(DOCUMENTS)?;
        let targets = paginate(parse::documents(&docs), "date", range);
        let mut out = Vec::with_capacity(targets.len());
        for doc in targets {
            let Some(id) = doc_id(&doc) else {
                continue; // no id → nothing to fetch; a redesign empties, not crashes
            };
            let bytes = c.download_doc(id, &file_name_of(&doc, id))?;
            out.push((doc, bytes));
        }
        Ok(out)
    })
}

/// The document's numeric id for the download API. documents/v1 carries `id`
/// as a string (see `parse::documents`); PennyMac's endpoint keys on the
/// numeric `docId`, so parse it back here.
fn doc_id(doc: &Value) -> Option<i64> {
    doc.get("id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
}

fn file_name_of(doc: &Value, id: i64) -> String {
    doc.get("file")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{id}.pdf"))
}

fn doc_name(doc: &Value) -> String {
    doc.get("name")
        .and_then(Value::as_str)
        .unwrap_or("document")
        .to_string()
}

fn saved_dto(doc: &Value, id: i64, path: &std::path::Path, bytes: usize) -> Value {
    json!({
        // documents/v1: `id` is a string (see parse::documents).
        "id": id.to_string(),
        "name": doc_name(doc),
        "category": doc.get("category").cloned().unwrap_or(Value::Null),
        "date": doc.get("date").cloned().unwrap_or(Value::Null),
        "file": file_name_of(doc, id),
        "path": path.display().to_string(),
        "bytes": bytes,
    })
}

fn saved_line(v: &Value) -> String {
    format!(
        "Saved {} → {} ({} bytes)",
        v.get("name").and_then(Value::as_str).unwrap_or("document"),
        v.get("path").and_then(Value::as_str).unwrap_or("?"),
        v.get("bytes").and_then(Value::as_u64).unwrap_or(0),
    )
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
