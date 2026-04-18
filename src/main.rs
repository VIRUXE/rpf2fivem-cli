mod archive;
mod converter;
mod manifest;
mod rpf;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rpf2fivem",
    version = env!("CARGO_PKG_VERSION"),
    about = "Convert GTA V .rpf archives to FiveM resource folders",
    arg_required_else_help = true,
)]
struct Cli {
    /// Path to archive (.zip/.rar/.7z), direct download URL, or mod page URL
    /// (e.g. https://www.gta5-mods.com/vehicles/gta-iv-feltzer)
    input: Option<String>,

    /// Resource name (default: detected streaming model name)
    #[arg(short, long)]
    name: Option<String>,

    /// Description written into fxmanifest.lua
    #[arg(short, long)]
    description: Option<String>,

    /// Output directory for resources
    #[arg(short, long, default_value = ".")]
    output: PathBuf,

    /// Combine multiple vehicles into a single resource folder
    #[arg(long)]
    combine: bool,

    /// Combined resource folder name (used with --combine)
    #[arg(long, default_value = "combined_vehicles")]
    combine_name: String,

    /// Overwrite the output resource folder if it already exists (skip prompt)
    #[arg(short = 'y', long = "yes")]
    overwrite: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(input) = cli.input {
        let name_explicit = cli.name.is_some();
        let resource_name = cli.name.unwrap_or_else(|| {
            // Provisional name; renamed to streaming model after extraction.
            converter::name_from_url(&input).unwrap_or_else(|| {
                use std::time::{SystemTime, UNIX_EPOCH};
                let n = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(12345678);
                format!("{n}")
            })
        });

        cmd_convert(
            input,
            resource_name,
            name_explicit,
            cli.description,
            cli.output,
            cli.combine,
            cli.combine_name,
            cli.overwrite,
        )
    } else {
        eprintln!("No input provided. Run with --help for usage.");
        std::process::exit(1);
    }
}

fn cmd_convert(
    input: String,
    resource_name: String,
    name_explicit: bool,
    description: Option<String>,
    output: PathBuf,
    combine: bool,
    combine_name: String,
    overwrite: bool,
) -> Result<()> {
    std::fs::create_dir_all(&output)?;

    let opts = converter::ConvertOptions {
        input: &input,
        resource_name: &resource_name,
        description: description.as_deref(),
        output_dir: &output,
        combined: combine,
        combined_name: &combine_name,
        overwrite,
    };

    let mut result = converter::convert(&opts)
        .with_context(|| format!("Conversion failed for: {}", input))?;

    // If the user did not pass -n, rename the resource folder to the
    // detected streaming model name (the .yft basename).
    if !name_explicit && !combine {
        if let Some(model) = &result.streaming_name {
            if model != &resource_name {
                let new_path = output.join(model);
                if new_path.exists() {
                    if overwrite {
                        if let Err(e) = std::fs::remove_dir_all(&new_path) {
                            eprintln!(
                                "[Worker] Could not remove {} for rename: {}",
                                new_path.display(),
                                e
                            );
                        } else if let Err(e) =
                            std::fs::rename(&result.resource_path, &new_path)
                        {
                            eprintln!("[Worker] Could not rename to {}: {}", new_path.display(), e);
                        } else {
                            result.resource_path = new_path;
                        }
                    } else {
                        eprintln!(
                            "[Worker] Target {} already exists, keeping name {}",
                            new_path.display(),
                            resource_name
                        );
                    }
                } else if let Err(e) = std::fs::rename(&result.resource_path, &new_path) {
                    eprintln!("[Worker] Could not rename to {}: {}", new_path.display(), e);
                } else {
                    result.resource_path = new_path;
                }
            }
        }
    }

    eprintln!(
        "{}",
        format!(
            "[Done] Resource: {} ({}ms)",
            result.resource_path.display(),
            result.elapsed_ms
        )
        .green()
    );

    if let Some(model) = &result.streaming_name {
        eprintln!("[Done] Streaming model: {}", model.cyan());
    }

    Ok(())
}
