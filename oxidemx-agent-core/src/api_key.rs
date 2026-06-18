//! API key loading — provider-agnostic: checks GEMINI_API_KEY first,
//! then ~/.config/oxidemx/gemini.key, then the legacy juhradial path.
//! Pure std::fs/std::env; no overlay dependency.

/// Load the Gemini API key from the environment or the user's config files.
pub fn load_api_key() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    let path = std::path::Path::new(&home).join(".config/oxidemx/gemini.key");
    if path.exists() {
        let key = std::fs::read_to_string(path)?;
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    let path_legacy = std::path::Path::new(&home).join(".config/juhradial/gemini.key");
    if path_legacy.exists() {
        let key = std::fs::read_to_string(path_legacy)?;
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    Err("Gemini API key not found. Please set GEMINI_API_KEY or save it in ~/.config/oxidemx/gemini.key".into())
}
