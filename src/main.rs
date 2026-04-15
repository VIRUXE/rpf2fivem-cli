mod archive;
mod converter;
mod manifest;
mod rpf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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

    /// Resource name (default: inferred from URL or filename)
    #[arg(short, long)]
    name: Option<String>,

    /// Description written into fxmanifest.lua
    #[arg(short, long)]
    description: Option<String>,

    /// Output directory for resources
    #[arg(short, long, default_value = "resources")]
    output: PathBuf,

    /// Combine multiple vehicles into a single resource folder
    #[arg(long)]
    combine: bool,

    /// Combined resource folder name (used with --combine)
    #[arg(long, default_value = "combined_vehicles")]
    combine_name: String,

    /// Path to keys directory (containing gtav_aes_key.dat etc.)
    #[arg(long, default_value = "keys")]
    keys: PathBuf,

    /// Generate QBX-Core vehicle list entry
    #[arg(long)]
    qbx: bool,

    /// Generate QB-Core vehicle list entry
    #[arg(long)]
    qbcore: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract GTA V crypto keys from GTA5.exe (required for encrypted RPFs)
    ExtractKeys {
        /// Path to GTA5.exe
        #[arg(long, default_value = "GTA5.exe")]
        exe: PathBuf,

        /// Directory to save extracted key files
        #[arg(short, long, default_value = "keys")]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::ExtractKeys { exe, output } => cmd_extract_keys(exe, output),
        }
    } else if let Some(input) = cli.input {
        let resource_name = cli.name.unwrap_or_else(|| {
            // Infer from URL slug or filename, fall back to timestamp nanos
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
            cli.description,
            cli.output,
            cli.combine,
            cli.combine_name,
            cli.keys,
            cli.qbx,
            cli.qbcore,
        )
    } else {
        // No input and no subcommand — clap's arg_required_else_help should
        // have already printed help, but guard just in case.
        eprintln!("No input provided. Run with --help for usage.");
        std::process::exit(1);
    }
}

fn cmd_convert(
    input: String,
    resource_name: String,
    description: Option<String>,
    output: PathBuf,
    combine: bool,
    combine_name: String,
    keys_path: PathBuf,
    qbx: bool,
    qbcore: bool,
) -> Result<()> {
    let keys = if keys_path.exists() {
        match rpf::keys::GtaKeys::load_from_path(&keys_path) {
            Ok(k) => {
                eprintln!("{}", "[Keys] Crypto keys loaded.".green());
                Some(k)
            }
            Err(e) => {
                eprintln!("{}: {}", "[Keys] Warning: could not load keys".yellow(), e);
                eprintln!("       Run `rpf2fivem extract-keys` to extract keys from GTA5.exe.");
                None
            }
        }
    } else {
        eprintln!(
            "{}",
            "[Keys] No keys directory found — RPF decryption disabled.".yellow()
        );
        eprintln!("       Run `rpf2fivem extract-keys --exe /path/to/GTA5.exe` first.");
        None
    };

    std::fs::create_dir_all(&output)?;

    let opts = converter::ConvertOptions {
        input: &input,
        resource_name: &resource_name,
        description: description.as_deref(),
        output_dir: &output,
        combined: combine,
        combined_name: &combine_name,
        keys: keys.as_ref(),
        qbx,
        qbcore,
    };

    let result = converter::convert(&opts)
        .with_context(|| format!("Conversion failed for: {}", input))?;

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
        if qbx || qbcore {
            handle_helper_output(&resource_name, Some(model.as_str()), qbx, qbcore)?;
        }
    } else if qbx || qbcore {
        handle_helper_output(&resource_name, None, qbx, qbcore)?;
    }

    Ok(())
}

fn handle_helper_output(
    resource_name: &str,
    streaming_name: Option<&str>,
    qbx: bool,
    qbcore: bool,
) -> Result<()> {
    let model = streaming_name.unwrap_or(resource_name);

    if qbx {
        append_file(
            "qbxcore_vehicles.txt",
            &format!(
                "{model} = {{\n    name = 'Unknown',\n    brand = 'Unknown',\n    model = '{model}',\n    price = 0,\n    category = 'Compacts',\n    type = 'automobile',\n    hash = '{model}',\n}},\n"
            ),
        )?;
        eprintln!("[Helper] Appended to qbxcore_vehicles.txt");
    }

    if qbcore {
        append_file(
            "qbcore_vehicles.txt",
            &format!(
                "{{ model = '{model}', name = 'Unknown', brand = 'Unknown', price = 0, category = 'Compacts', type = 'automobile', shop = 'none' }},\n"
            ),
        )?;
        eprintln!("[Helper] Appended to qbcore_vehicles.txt");
    }

    Ok(())
}

fn append_file(filename: &str, content: &str) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(filename)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

fn cmd_extract_keys(exe_path: PathBuf, output_path: PathBuf) -> Result<()> {
    if !exe_path.exists() {
        anyhow::bail!(
            "GTA5.exe not found at '{}'. Specify path with --exe.",
            exe_path.display()
        );
    }

    eprintln!(
        "{}",
        format!("[Keys] Extracting from {}...", exe_path.display()).cyan()
    );

    rpf::keys::GtaKeys::extract_from_exe(&exe_path, Some(&output_path))
        .context("Key extraction failed")?;

    eprintln!(
        "{}",
        format!("[Keys] Keys saved to {}", output_path.display()).green()
    );

    Ok(())
}
