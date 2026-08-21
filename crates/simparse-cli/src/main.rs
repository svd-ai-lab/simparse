use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use simparse_core::{
    InspectOptions, ScanOptions, SimFormat, inspect_path, scan_paths, summarize_result,
};

#[derive(Debug, Parser)]
#[command(name = "simparse")]
#[command(about = "Inspect simulation project and case files.")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inspect {
        path: PathBuf,
        #[arg(long, default_value = "auto")]
        format: String,
        #[arg(long)]
        json: bool,
        #[arg(long, requires = "json")]
        summary: bool,
        #[arg(long)]
        include_paths: bool,
        #[arg(long, default_value_t = 2 * 1024 * 1024)]
        max_text_bytes: usize,
    },
    Scan {
        path: PathBuf,
        #[arg(long)]
        jsonl: bool,
        #[arg(
            long,
            default_value = "*.mph,*.inp,*.inc,*.cas.h5,*.msh.h5,*.aedt,*.aedtz"
        )]
        include: String,
        #[arg(long)]
        include_paths: bool,
        #[arg(long, default_value_t = true)]
        recursive: bool,
        #[arg(long, default_value_t = 2 * 1024 * 1024)]
        max_text_bytes: usize,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect {
            path,
            format,
            json,
            summary,
            include_paths,
            max_text_bytes,
        } => {
            let options = InspectOptions {
                format: parse_format(&format)?,
                include_paths,
                max_text_bytes,
            };
            let result = inspect_path(&path, options)
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if summary {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summarize_result(&result))?
                );
            } else if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}: {}", result.file_name, result.format);
                println!("{}", serde_json::to_string_pretty(&result.summary)?);
            }
        }
        Command::Scan {
            path,
            jsonl,
            include,
            include_paths,
            recursive,
            max_text_bytes,
        } => {
            let options = ScanOptions {
                recursive,
                include_paths,
                includes: include
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
                inspect: InspectOptions {
                    format: None,
                    include_paths,
                    max_text_bytes,
                },
            };
            let results = scan_paths(&[path], options)?;
            if jsonl {
                for result in results {
                    println!("{}", serde_json::to_string(&result)?);
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&results)?);
            }
        }
    }
    Ok(())
}

fn parse_format(value: &str) -> anyhow::Result<Option<SimFormat>> {
    if value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    match value.parse::<SimFormat>() {
        Ok(format) => Ok(Some(format)),
        Err(err) => bail!(err),
    }
}
