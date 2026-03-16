use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub struct ExtractLimits {
    pub max_file_size: usize,
    pub max_total_size: usize,
    pub max_entry_count: usize,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024,      // 10MB per file
            max_total_size: 500 * 1024 * 1024,     // 500MB total
            max_entry_count: 5000,
        }
    }
}

pub struct BulkUploadResult {
    pub uploaded: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

impl serde::Serialize for BulkUploadResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("BulkUploadResult", 3)?;
        s.serialize_field("uploaded", &self.uploaded)?;
        s.serialize_field("skipped", &self.skipped)?;
        s.serialize_field("errors", &self.errors)?;
        s.end()
    }
}

/// Detect common root directory prefix shared by all zip entries.
/// E.g. if all entries start with "test-skill/", returns Some("test-skill/").
fn detect_common_prefix(names: &[String]) -> Option<String> {
    let non_dir_names: Vec<&str> = names.iter()
        .filter(|n| !n.ends_with('/'))
        .map(|n| n.as_str())
        .collect();

    if non_dir_names.is_empty() {
        return None;
    }

    // Find first path component of the first entry
    let first = non_dir_names[0];
    let prefix = match first.find('/') {
        Some(idx) => &first[..=idx], // e.g. "test-skill/"
        None => return None,          // no directory structure
    };

    // Check if ALL entries share this prefix
    if non_dir_names.iter().all(|n| n.starts_with(prefix)) {
        Some(prefix.to_string())
    } else {
        None
    }
}

/// Extract a zip from bytes into a HashMap<relative_path, text_content>.
/// Skips binary files (non-UTF-8). Strips common root prefix.
pub fn extract_zip_to_map(data: &[u8], limits: &ExtractLimits) -> Result<HashMap<String, String>, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {e}"))?;

    if archive.len() > limits.max_entry_count {
        return Err(format!("Too many entries: {} (max {})", archive.len(), limits.max_entry_count));
    }

    // Collect entry names for prefix detection
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
        .collect();
    let prefix = detect_common_prefix(&names);

    let mut files = HashMap::new();
    let mut total_size: usize = 0;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("Zip entry error: {e}"))?;
        let raw_name = entry.name().to_string();

        // Skip directories
        if raw_name.ends_with('/') {
            continue;
        }

        // Strip common prefix
        let name = if let Some(ref pfx) = prefix {
            raw_name.strip_prefix(pfx.as_str()).unwrap_or(&raw_name).to_string()
        } else {
            raw_name.clone()
        };

        // Skip empty names after stripping
        if name.is_empty() {
            continue;
        }

        // Path safety
        if name.contains("..") || name.starts_with('/') {
            continue;
        }

        // Size check
        let size = entry.size() as usize;
        if size > limits.max_file_size {
            continue; // skip oversized
        }
        total_size += size;
        if total_size > limits.max_total_size {
            return Err(format!("Total extracted size exceeds limit ({}MB)", limits.max_total_size / 1024 / 1024));
        }

        // Read content
        let mut buf = Vec::with_capacity(size);
        entry.read_to_end(&mut buf).map_err(|e| format!("Read error for {name}: {e}"))?;

        // Only text files for skills
        match String::from_utf8(buf) {
            Ok(text) => { files.insert(name, text); }
            Err(_) => {
                tracing::debug!(file = %name, "Skipped binary file in skill zip");
            }
        }
    }

    Ok(files)
}

/// Extract a zip from bytes to a directory on disk.
/// Supports binary files. Strips common root prefix.
pub async fn extract_zip_to_dir(
    data: &[u8],
    dest: &Path,
    prefix: &str,
    limits: &ExtractLimits,
) -> Result<BulkUploadResult, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {e}"))?;

    if archive.len() > limits.max_entry_count {
        return Err(format!("Too many entries: {} (max {})", archive.len(), limits.max_entry_count));
    }

    // Collect entry names for prefix detection
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
        .collect();
    let zip_prefix = detect_common_prefix(&names);

    let mut result = BulkUploadResult {
        uploaded: 0,
        skipped: 0,
        errors: Vec::new(),
    };
    let mut total_size: usize = 0;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                result.errors.push(format!("Entry {i}: {e}"));
                continue;
            }
        };
        let raw_name = entry.name().to_string();

        // Skip directories
        if raw_name.ends_with('/') {
            continue;
        }

        // Strip common zip prefix
        let stripped = if let Some(ref pfx) = zip_prefix {
            raw_name.strip_prefix(pfx.as_str()).unwrap_or(&raw_name).to_string()
        } else {
            raw_name.clone()
        };

        if stripped.is_empty() {
            continue;
        }

        // Path safety
        if stripped.contains("..") || stripped.starts_with('/') {
            result.skipped += 1;
            continue;
        }

        // Size checks
        let size = entry.size() as usize;
        if size > limits.max_file_size {
            result.skipped += 1;
            continue;
        }
        total_size += size;
        if total_size > limits.max_total_size {
            result.errors.push(format!("Total size limit exceeded ({}MB)", limits.max_total_size / 1024 / 1024));
            break;
        }

        // Build destination path with optional user prefix
        let file_path = if prefix.is_empty() {
            dest.join(&stripped)
        } else {
            dest.join(prefix).join(&stripped)
        };

        // Create parent dirs
        if let Some(parent) = file_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                result.errors.push(format!("{stripped}: mkdir failed: {e}"));
                continue;
            }
        }

        // Read and write
        let mut buf = Vec::with_capacity(size);
        if let Err(e) = entry.read_to_end(&mut buf) {
            result.errors.push(format!("{stripped}: read failed: {e}"));
            continue;
        }

        match tokio::fs::write(&file_path, &buf).await {
            Ok(()) => result.uploaded += 1,
            Err(e) => result.errors.push(format!("{stripped}: write failed: {e}")),
        }
    }

    Ok(result)
}
