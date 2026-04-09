use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    archive,
    manifest,
    rpf::{keys::GtaKeys, RpfArchive},
};

#[allow(dead_code)]
pub struct ConvertOptions<'a> {
    pub input: &'a str,
    pub resource_name: &'a str,
    pub output_dir: &'a Path,
    pub combined: bool,
    pub combined_name: &'a str,
    pub keys: Option<&'a GtaKeys>,
    pub qbx: bool,
    pub qbcore: bool,
}

pub struct ConvertResult {
    pub resource_path: PathBuf,
    pub streaming_name: Option<String>,
    pub elapsed_ms: u128,
}

/// Stream folder file extensions (go into stream/)
const STREAM_EXTS: &[&str] = &["yft", "ytd", "ydr"];
/// Data folder file extensions (go into data/)
const DATA_EXTS: &[&str] = &["meta"];
/// Additional data extensions that are also valid
const EXTRA_DATA_EXTS: &[&str] = &["xml"];

pub fn convert(opts: &ConvertOptions) -> Result<ConvertResult> {
    let timer = Instant::now();
    let cache = tempfile::tempdir().context("Failed to create temp dir")?;
    let cache_path = cache.path();

    eprintln!("[Worker] Processing: {}", opts.input);

    // Step 1: Obtain the archive (download or copy)
    let archive_path = acquire_archive(opts.input, cache_path)?;

    // Step 2: Extract archive
    eprintln!("[Archive] Extracting {}...", archive_path.display());
    let extract_dir = cache_path.join("unpack");
    archive::extract(&archive_path, &extract_dir)?;

    // Step 3: Find and parse RPF files
    eprintln!("[RPF] Searching for .rpf files...");
    let rpf_files = archive::find_rpf_files(&extract_dir);
    if rpf_files.is_empty() {
        eprintln!("[RPF] Warning: no .rpf files found in {}", opts.input);
    }

    // Step 4: Set up output structure
    let (resource_dir, stream_dir, data_dir) = setup_resource_dirs(
        opts.output_dir,
        opts.resource_name,
        opts.combined,
        opts.combined_name,
    )?;

    // Step 5: Extract relevant files from each RPF
    let mut streaming_name: Option<String> = None;

    for rpf_path in &rpf_files {
        eprintln!("[RPF] Parsing {}...", rpf_path.display());
        let rpf_data = fs::read(rpf_path)
            .with_context(|| format!("Cannot read {}", rpf_path.display()))?;

        let rpf_filename = rpf_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("dlc.rpf");

        let archive = match RpfArchive::parse_from_bytes(
            &rpf_data,
            rpf_filename,
            0,
            opts.keys,
        ) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[RPF] Skipping {}: {}", rpf_path.display(), e);
                continue;
            }
        };

        archive.extract_all(&rpf_data, opts.keys, |name, data| {
            let ext = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let basename = Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(name);

            if STREAM_EXTS.contains(&ext) {
                // Fix resource header bytes if needed (byte 3 → '7')
                let mut file_data = data;
                if ext == "ytd" || ext == "yft" {
                    fix_resource_header(&mut file_data);
                }

                // Detect streaming name from ytd/yft pairing
                if ext == "ytd" && !basename.ends_with("+hi.ytd") {
                    let base = basename.trim_end_matches(".ytd");
                    let yft_path = stream_dir.join(format!("{}.yft", base));
                    if yft_path.exists() {
                        streaming_name = Some(base.to_string());
                        eprintln!("[RPF] Detected streaming name: {}", base);
                    }
                } else if ext == "yft" {
                    let base = basename.trim_end_matches(".yft");
                    let ytd_path = stream_dir.join(format!("{}.ytd", base));
                    if ytd_path.exists() {
                        streaming_name = Some(base.to_string());
                        eprintln!("[RPF] Detected streaming name: {}", base);
                    }
                }

                let dest = stream_dir.join(basename);
                if let Err(e) = fs::write(&dest, &file_data) {
                    eprintln!("[Worker] Failed to write {}: {}", dest.display(), e);
                } else {
                    eprintln!("[Worker] -> stream/{}", basename);
                }
            } else if DATA_EXTS.contains(&ext) || EXTRA_DATA_EXTS.contains(&ext) {
                // Only accept .meta files that are relevant vehicle data
                if is_vehicle_meta(name) {
                    let dest = data_dir.join(basename);
                    if let Err(e) = fs::write(&dest, &data) {
                        eprintln!("[Worker] Failed to write {}: {}", dest.display(), e);
                    } else {
                        eprintln!("[Worker] -> data/{}", basename);
                    }
                }
            }
        })?;
    }

    let elapsed_ms = timer.elapsed().as_millis();
    eprintln!("[Worker] Done in {}ms", elapsed_ms);

    Ok(ConvertResult {
        resource_path: resource_dir,
        streaming_name,
        elapsed_ms,
    })
}

fn acquire_archive(input: &str, cache_dir: &Path) -> Result<PathBuf> {
    if input.starts_with("https://") || input.starts_with("http://") {
        let filename = input
            .split('/')
            .last()
            .unwrap_or("download.zip");
        let dest = cache_dir.join(filename);
        archive::download(input, &dest)?;
        Ok(dest)
    } else {
        let src = Path::new(input);
        let dest = cache_dir.join(
            src.file_name()
                .context("Input has no filename")?,
        );
        fs::copy(src, &dest)?;
        Ok(dest)
    }
}

fn setup_resource_dirs(
    output_dir: &Path,
    resource_name: &str,
    combined: bool,
    combined_name: &str,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let resource_dir = if combined {
        output_dir.join(combined_name)
    } else {
        output_dir.join(resource_name)
    };

    let stream_dir = if combined {
        resource_dir.join("stream").join(resource_name)
    } else {
        resource_dir.join("stream")
    };

    let data_dir = if combined {
        resource_dir.join("data").join(resource_name)
    } else {
        resource_dir.join("data")
    };

    fs::create_dir_all(&stream_dir)?;
    fs::create_dir_all(&data_dir)?;

    // Write fxmanifest.lua if not already present
    let manifest_path = resource_dir.join("fxmanifest.lua");
    if !manifest_path.exists() {
        let content = if combined {
            manifest::combined()
        } else {
            manifest::single()
        };
        fs::write(&manifest_path, content)?;
    }

    Ok((resource_dir, stream_dir, data_dir))
}

/// Fix the resource header so byte index 3 is '7' (0x37).
/// This corrects a quirk in some extracted files.
fn fix_resource_header(data: &mut Vec<u8>) {
    if data.len() >= 4 {
        data[3] = b'7';
    }
}

/// Check if a .meta filename is a vehicle-relevant data file.
fn is_vehicle_meta(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        Path::new(&lower)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(""),
        "handling.meta"
            | "vehicles.meta"
            | "vehiclelayouts.meta"
            | "carcols.meta"
            | "carvariations.meta"
            | "dlctext.meta"
            | "contentunlocks.meta"
            | "vehiclemodelsets.meta"
    )
}
