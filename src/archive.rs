use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Extract a zip/rar/7z archive, returning a list of extracted file paths.
pub fn extract(archive_path: &Path, output_dir: &Path) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(output_dir)?;

    let ext = archive_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "zip" => extract_zip(archive_path, output_dir),
        "7z" => extract_7z(archive_path, output_dir),
        "rar" => extract_rar(archive_path, output_dir),
        _ => bail!("Unsupported archive format: .{}", ext),
    }
}

fn extract_zip(archive_path: &Path, output_dir: &Path) -> Result<Vec<PathBuf>> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("Cannot open zip: {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut extracted = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let out_path = output_dir.join(&name);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        fs::write(&out_path, &data)?;
        extracted.push(out_path);
    }

    Ok(extracted)
}

fn extract_7z(archive_path: &Path, output_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut extracted = Vec::new();
    sevenz_rust::decompress_file(archive_path, output_dir)
        .with_context(|| format!("Failed to extract 7z: {}", archive_path.display()))?;

    // Collect all extracted files
    collect_files(output_dir, &mut extracted);
    Ok(extracted)
}

fn extract_rar(archive_path: &Path, output_dir: &Path) -> Result<Vec<PathBuf>> {
    // RAR extraction using the system unrar or a Rust crate
    // We use the zip crate's generic approach - for RAR we try via system command
    let status = std::process::Command::new("unrar")
        .args(["x", "-y", "-o+"])
        .arg(archive_path)
        .arg(output_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            let mut extracted = Vec::new();
            collect_files(output_dir, &mut extracted);
            Ok(extracted)
        }
        Ok(s) => bail!("unrar exited with status {}", s),
        Err(_) => {
            // Try extracting via zip (some "rar" files are actually zip)
            extract_zip(archive_path, output_dir)
                .context("RAR extraction failed (unrar not found, attempted as zip)")
        }
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
}

/// Find all .rpf files in a directory tree.
pub fn find_rpf_files(dir: &Path) -> Vec<PathBuf> {
    let mut rpfs = Vec::new();
    collect_by_ext(dir, "rpf", &mut rpfs);
    rpfs
}

fn collect_by_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_by_ext(&path, ext, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                out.push(path);
            }
        }
    }
}

/// Download a file from a URL, returning local path.
pub fn download(url: &str, dest: &Path) -> Result<()> {
    eprintln!("[Download] Fetching {}...", url);
    let response = reqwest::blocking::get(url)
        .with_context(|| format!("HTTP request failed: {}", url))?;

    if !response.status().is_success() {
        bail!("HTTP {} for {}", response.status(), url);
    }

    let bytes = response.bytes()?;
    fs::write(dest, &bytes)?;
    eprintln!("[Download] Saved {} bytes to {}", bytes.len(), dest.display());
    Ok(())
}
