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
    pub description: Option<&'a str>,
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

    // Step 4: Set up output structure
    let (resource_dir, stream_dir, data_dir) = setup_resource_dirs(
        opts.output_dir,
        opts.resource_name,
        opts.combined,
        opts.combined_name,
    )?;

    // Step 5: Extract relevant files from each RPF
    let mut streaming_name: Option<String> = None;
    let mut written_meta: Vec<String> = Vec::new();

    if rpf_files.is_empty() {
        // No RPF found — copy loose stream/data files directly from the archive
        eprintln!("[RPF] No .rpf files found, looking for loose stream files...");
        copy_loose_files(&extract_dir, &stream_dir, &data_dir, &mut streaming_name, &mut written_meta);
    }

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
                    let _ = fs::create_dir_all(&data_dir);
                    if let Err(e) = fs::write(&dest, &data) {
                        eprintln!("[Worker] Failed to write {}: {}", dest.display(), e);
                    } else {
                        eprintln!("[Worker] -> data/{}", basename);
                        written_meta.push(basename.to_string());
                    }
                }
            }
        })?;
    }

    // Step 6: Write fxmanifest.lua now that we know which meta files are present
    let meta_refs: Vec<&str> = written_meta.iter().map(|s| s.as_str()).collect();
    let manifest_content = if opts.combined {
        manifest::combined(&meta_refs, opts.description)
    } else {
        manifest::single(&meta_refs, opts.description)
    };
    fs::write(resource_dir.join("fxmanifest.lua"), &manifest_content)?;

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
        let download_url = resolve_download_url(input)?;
        archive::download(&download_url, cache_dir)
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

/// If `url` points to a mod page (e.g. gta5-mods.com/vehicles/slug),
/// scrape it to find the actual file download URL.
/// Direct file URLs (ending in .zip/.rar/.7z) are returned unchanged.
fn resolve_download_url(url: &str) -> Result<String> {
    // Already a direct file link — nothing to do
    let lower = url.to_lowercase();
    if lower.ends_with(".zip") || lower.ends_with(".rar") || lower.ends_with(".7z") {
        return Ok(url.to_string());
    }

    if url.contains("gta5-mods.com") {
        return resolve_gta5mods(url);
    }

    // Unknown site — try as-is and let the downloader handle it
    Ok(url.to_string())
}

fn resolve_gta5mods(page_url: &str) -> Result<String> {
    eprintln!("[Download] Detected gta5-mods.com page, resolving download link...");

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    let base = extract_url_base(page_url)?;

    // Step 1: fetch the mod page → find the /download/ID link
    let mod_html = client
        .get(page_url)
        .send()
        .with_context(|| format!("Failed to fetch mod page: {}", page_url))?
        .text()?;

    let download_page_url = match find_download_href(&mod_html) {
        Some(href) if href.starts_with("http") => href,
        Some(href) => format!("{}{}", base, href),
        None => {
            // Fallback: append /download
            format!("{}/download", page_url.trim_end_matches('/'))
        }
    };

    eprintln!("[Download] Download page: {}", download_page_url);

    // Step 2: fetch the download page → find the actual CDN file URL
    let dl_html = client
        .get(&download_page_url)
        .header("Referer", page_url)
        .send()
        .with_context(|| format!("Failed to fetch download page: {}", download_page_url))?
        .text()?;

    if let Some(file_url) = find_download_href(&dl_html) {
        let full_url = if file_url.starts_with("http") {
            file_url
        } else {
            format!("{}{}", base, file_url)
        };
        eprintln!("[Download] Resolved file URL: {}", full_url);
        return Ok(full_url);
    }

    anyhow::bail!(
        "Could not find a direct download URL on the download page.\n\
         Try downloading the file manually and pass the local path."
    )
}

/// Scan HTML for the primary download href.
/// Matches URLs containing "/download" in the path, e.g.:
///   /vehicles/slug/download
///   /vehicles/slug/download/5086
/// Prefers the btn-download anchor when present.
fn find_download_href(html: &str) -> Option<String> {
    // First pass: look for the btn-download class anchor which is the main button
    if let Some(pos) = html.find("btn-download") {
        // Walk backwards from btn-download to find its href
        let before = &html[..pos];
        if let Some(anchor_start) = before.rfind("<a ") {
            let anchor_chunk = &html[anchor_start..anchor_start + 300];
            if let Some(href_val) = extract_href_from_tag(anchor_chunk) {
                return Some(href_val);
            }
        }
    }

    // Second pass: any href containing /download
    let mut search = html;
    while let Some(pos) = search.find("href=\"") {
        let rest = &search[pos + 6..];
        if let Some(end) = rest.find('"') {
            let href = &rest[..end];
            if href.contains("/download") {
                return Some(href.to_string());
            }
        }
        search = &search[pos + 6..];
    }
    None
}

fn extract_href_from_tag(tag_html: &str) -> Option<String> {
    let mut search = tag_html;
    while let Some(pos) = search.find("href=\"") {
        let rest = &search[pos + 6..];
        if let Some(end) = rest.find('"') {
            let href = &rest[..end];
            if !href.is_empty() {
                return Some(href.to_string());
            }
        }
        search = &search[pos + 6..];
    }
    None
}

fn extract_url_base(url: &str) -> Result<String> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .context("URL missing scheme")?;
    let host = without_scheme.split('/').next().context("URL missing host")?;
    let scheme = if url.starts_with("https://") { "https" } else { "http" };
    Ok(format!("{}://{}", scheme, host))
}

/// Infer a resource name from a URL (uses the last meaningful path segment).
pub fn name_from_url(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .split('/')
        .last()
        .filter(|s| !s.is_empty() && *s != "download")
        .map(|s| s.to_string())
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
    // data_dir and fxmanifest.lua are written after extraction

    Ok((resource_dir, stream_dir, data_dir))
}

/// Copy loose stream/data files directly from the extracted archive directory.
/// Used when no RPF is present (the mod ships raw .yft/.ytd/.meta files).
fn copy_loose_files(
    extract_dir: &Path,
    stream_dir: &Path,
    data_dir: &Path,
    streaming_name: &mut Option<String>,
    written_meta: &mut Vec<String>,
) {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files_by_exts(extract_dir, &["yft", "ytd", "ydr", "meta"], &mut files);

    for path in &files {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let basename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if STREAM_EXTS.contains(&ext.as_str()) {
            // Skip +hi variants for streaming name detection (keep _hi.yft as a separate file)
            if ext == "ytd" && !basename.ends_with("+hi.ytd") {
                let base = basename.trim_end_matches(".ytd");
                let yft = stream_dir.join(format!("{}.yft", base));
                if yft.exists() {
                    *streaming_name = Some(base.to_string());
                    eprintln!("[Loose] Detected streaming name: {}", base);
                }
            } else if ext == "yft" && !basename.ends_with("_hi.yft") {
                let base = basename.trim_end_matches(".yft");
                let ytd = stream_dir.join(format!("{}.ytd", base));
                if ytd.exists() {
                    *streaming_name = Some(base.to_string());
                    eprintln!("[Loose] Detected streaming name: {}", base);
                }
            }

            let mut data = match fs::read(&path) {
                Ok(d) => d,
                Err(e) => { eprintln!("[Loose] Cannot read {}: {}", path.display(), e); continue; }
            };
            if ext == "ytd" || ext == "yft" {
                fix_resource_header(&mut data);
            }
            let dest = stream_dir.join(&basename);
            if let Err(e) = fs::write(&dest, &data) {
                eprintln!("[Loose] Failed to write {}: {}", dest.display(), e);
            } else {
                eprintln!("[Worker] -> stream/{}", basename);
            }
        } else if ext == "meta" && is_vehicle_meta(&basename) {
            let data = match fs::read(&path) {
                Ok(d) => d,
                Err(e) => { eprintln!("[Loose] Cannot read {}: {}", path.display(), e); continue; }
            };
            let dest = data_dir.join(&basename);
            let _ = fs::create_dir_all(&data_dir);
            if let Err(e) = fs::write(&dest, &data) {
                eprintln!("[Loose] Failed to write {}: {}", dest.display(), e);
            } else {
                eprintln!("[Worker] -> data/{}", basename);
                written_meta.push(basename.clone());
            }
        }
    }

    if files.is_empty() {
        eprintln!("[Loose] Warning: no stream or data files found in archive.");
    }
}

fn collect_files_by_exts(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let mut all: Vec<PathBuf> = Vec::new();
    collect_files_by_exts_recursive(dir, exts, &mut all);

    // Deduplicate by basename: keep the shallowest path (fewest components)
    let mut seen: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    for path in all {
        let basename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        let depth = path.components().count();
        let entry = seen.entry(basename).or_insert_with(|| path.clone());
        if path.components().count() < entry.components().count() {
            *entry = path;
        }
        let _ = depth;
    }
    out.extend(seen.into_values());
}

fn collect_files_by_exts_recursive(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_by_exts_recursive(&path, exts, out);
            } else if let Some(e) = path.extension().and_then(|e| e.to_str()) {
                if exts.contains(&e.to_lowercase().as_str()) {
                    out.push(path);
                }
            }
        }
    }
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
