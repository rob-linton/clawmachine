/// Guess a Content-Type string from a file extension.
///
/// Used by both workspace file serving and chat artifact download. Falls back
/// to `application/octet-stream` for unknown extensions so the browser will
/// download rather than try to render.
pub fn mime_from_extension(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tar" => "application/gzip",
        "yaml" | "yml" => "text/yaml",
        "toml" => "text/toml",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "dart" => "text/x-dart",
        "sh" => "text/x-shellscript",
        _ => "application/octet-stream",
    }
    .to_string()
}
