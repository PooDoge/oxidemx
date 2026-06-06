use gemini_client_api::gemini::ask::Gemini;
use gemini_client_api::gemini::types::sessions::Session;
use gemini_client_api::gemini::types::request::Tool;
use gemini_client_api::{gemini_function, gemini_schema};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use std::sync::Mutex;
use once_cell::sync::Lazy;

// =============================================================================
// GLOBAL CHANNELS FOR ASYNC TOOL-TO-UI COMMUNICATION
// =============================================================================

pub struct PendingQuestion {
    pub question: String,
    pub options: Vec<String>,
    pub response_tx: mpsc::Sender<String>,
}

/// Channel to send pending multiple choice questions to the UI event loop.
pub static QUESTION_TX: Lazy<Mutex<Option<mpsc::Sender<PendingQuestion>>>> = Lazy::new(|| Mutex::new(None));

/// Channel to notify the UI loop of configuration changes made by the agent.
pub static CONFIG_CHANGED_TX: Lazy<Mutex<Option<mpsc::Sender<String>>>> = Lazy::new(|| Mutex::new(None));

// =============================================================================
// STRUCTURED SCHEMAS FOR JSON MODE (THEMING, SLICES, PAGES)
// =============================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
#[gemini_schema]
pub struct GeneratedSlice {
    /// The label/text displayed on the radial menu slice (e.g. "Terminal")
    pub label: String,
    /// The type of action: "exec", "settings", or "submenu"
    pub action_type: String,
    /// The shell command to execute if type is "exec"
    pub command: String,
    /// The color theme for the slice (e.g. "green", "teal", "mauve", "blue", "peach", "pink", "sapphire", "lavender")
    pub color: String,
    /// The symbolic icon name (e.g., "utilities-terminal-symbolic", "folder-symbolic")
    pub icon: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[gemini_schema]
pub struct GeneratedPage {
    /// The name of the radial menu page (e.g. "Default", "Development")
    pub name: String,
    /// List of slices on this page (up to 8 slices)
    pub slices: Vec<GeneratedSlice>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[gemini_schema]
pub struct ThemeColorsOverride {
    /// Dominant background color (hex #rrggbb)
    pub base: String,
    /// Surface color for panels (hex #rrggbb)
    pub surface: String,
    /// Text color (hex #rrggbb)
    pub text: String,
    /// Accent color for highlighted items (hex #rrggbb)
    pub accent: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[gemini_schema]
pub struct GeneratedTheme {
    /// A clean name for the theme
    pub name: String,
    /// Whether this is a dark theme
    pub is_dark: bool,
    /// Theme colors mapping
    pub colors: ThemeColorsOverride,
}

// =============================================================================
// CONFIGURATION RESOLVERS
// =============================================================================

fn get_config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    std::path::Path::new(&home).join(".config/oxidemx/config.json")
}

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

// =============================================================================
// CLIENT FACTORY & AGENT BUILDER
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    GeneralChat,
    SettingsCustomizer,
}

pub fn create_gemini_client(
    api_key: &str,
    mode: AgentMode,
) -> Gemini {
    let model = "gemini-2.5-flash";
    
    match mode {
        AgentMode::GeneralChat => {
            let sys_prompt = "You are OxideMX-AI, a helpful conversational desktop assistant. \
                              You can answer questions, explain concepts, and query the web to ground your responses in real-time. \
                              Keep your responses concise and format them in markdown.";
            
            Gemini::new(api_key, model, Some(sys_prompt.into()))
                .set_tools(vec![
                    Tool::FunctionDeclarations(vec![
                        google_search::gemini_schema(),
                    ])
                ])
        }
        AgentMode::SettingsCustomizer => {
            let sys_prompt = "You are the OxideMX Settings Customizer. You specialize in configuring \
                              the OxideMX circular radial menu overlay, mouse remapping shortcuts, visual themes, and animation curves. \
                              You can read and modify the active layout config. When generating themes, layouts, or list recommendations, \
                              you can output structures in JSON matching the specified schemas. \
                              If you need to make changes, call the set_menu_config tool. \
                              If you have questions with multiple choice options, call the ask_multiple_choice_question tool. \
                              Keep your text replies clean, direct, and focused on layout modification.";
            
            Gemini::new(api_key, model, Some(sys_prompt.into()))
                .set_tools(vec![
                    Tool::FunctionDeclarations(vec![
                        get_menu_config::gemini_schema(),
                        set_menu_config::gemini_schema(),
                        list_system_apps::gemini_schema(),
                        google_search::gemini_schema(),
                        ask_multiple_choice_question::gemini_schema(),
                    ])
                ])
        }
    }
}

// =============================================================================
// ASYNC AGENT TOOLS (FUNCTIONS)
// =============================================================================

#[gemini_function]
/// Retrieve the current OxideMX radial menu layout, animation curves, and mouse button configuration.
async fn get_menu_config() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let path = get_config_path();
    if !path.exists() {
        // Fall back to shared default config
        let default_bytes = include_str!("../../oxidemx-shared/default-config.json");
        return Ok(default_bytes.to_string());
    }
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(content)
}

#[gemini_function]
/// Overwrite the current OxideMX radial menu configuration with a new JSON setup.
/// Use this to save changes to themes, layout slices, custom pages, or animation speeds.
async fn set_menu_config(
    /// The complete new configuration JSON string
    config_json: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let _: serde_json::Value = serde_json::from_str(&config_json)?;
    let path = get_config_path();
    
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    
    // Save previous config back-channel to the UI for undo stacking
    let tx_lock = CONFIG_CHANGED_TX.lock().unwrap();
    if let Some(ref tx) = *tx_lock {
        let _ = tx.send(config_json.clone()).await;
    }
    
    tokio::fs::write(&path, &config_json).await?;
    Ok("Configuration saved successfully".to_string())
}

#[gemini_function]
/// Scan the host system's desktop directories to list installed applications, commands, and icons.
/// Helpful for recommending executables to bind to custom slices.
async fn list_system_apps() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut apps = Vec::new();
    let dirs = vec![
        "/usr/share/applications",
        "/usr/local/share/applications",
    ];
    
    let home = std::env::var("HOME").unwrap_or_default();
    let user_apps_dir = format!("{}/.local/share/applications", home);
    let mut all_dirs = dirs;
    if !home.is_empty() {
        all_dirs.push(&user_apps_dir);
    }
    
    for dir_path in all_dirs {
        let path = std::path::Path::new(&dir_path);
        if !path.exists() {
            continue;
        }
        let mut entries = tokio::fs::read_dir(path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if name_str.ends_with(".desktop") {
                if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                    let mut name = None;
                    let mut exec = None;
                    let mut icon = None;
                    let mut categories = None;
                    
                    for line in content.lines() {
                        if line.starts_with("Name=") && name.is_none() {
                            name = Some(line.strip_prefix("Name=").unwrap().to_string());
                        } else if line.starts_with("Exec=") && exec.is_none() {
                            exec = Some(line.strip_prefix("Exec=").unwrap().to_string());
                        } else if line.starts_with("Icon=") && icon.is_none() {
                            icon = Some(line.strip_prefix("Icon=").unwrap().to_string());
                        } else if line.starts_with("Categories=") && categories.is_none() {
                            categories = Some(line.strip_prefix("Categories=").unwrap().to_string());
                        }
                    }
                    
                    if let (Some(n), Some(e)) = (name, exec) {
                        apps.push(json!({
                            "name": n,
                            "exec": e,
                            "icon": icon.unwrap_or_default(),
                            "categories": categories.unwrap_or_default()
                        }));
                    }
                }
            }
        }
    }
    
    Ok(serde_json::to_string_pretty(&apps)?)
}

#[gemini_function]
/// Search the web for real-time information using DuckDuckGo.
/// Returns a list of titles, links, and snippets.
async fn google_search(
    /// The search query terms
    query: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    
    // URL encode query
    let encoded_query = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);
    
    let response = client.get(&url).send().await?.text().await?;
    let results = parse_ddg_html(&response);
    
    if results.is_empty() {
        Ok("No search results found.".to_string())
    } else {
        Ok(serde_json::to_string_pretty(&results)?)
    }
}

#[gemini_function]
/// Ask the user a clarifying multiple-choice question.
/// Used when there are multiple valid options or parameters to clarify.
async fn ask_multiple_choice_question(
    /// The question text to present
    question: String,
    /// The list of choices/options the user can click
    options: Vec<String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let tx_lock = QUESTION_TX.lock().unwrap();
    if let Some(ref tx) = *tx_lock {
        let (resp_tx, mut resp_rx) = mpsc::channel(1);
        let pending = PendingQuestion {
            question,
            options,
            response_tx: resp_tx,
        };
        tx.send(pending).await?;
        
        // Wait asynchronously for response from the UI main loop
        if let Some(answer) = resp_rx.recv().await {
            Ok(answer)
        } else {
            Err("Response channel closed".into())
        }
    } else {
        Err("Question channel not initialized".into())
    }
}

// =============================================================================
// HELPER SCANNERS & CLEANERS
// =============================================================================

fn parse_ddg_html(html: &str) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    let parts: Vec<&str> = html.split("class=\"result").collect();
    
    for part in parts.iter().skip(1) {
        let a_class = "result__a";
        if let Some(a_pos) = part.find(a_class) {
            let sub = &part[a_pos..];
            if let Some(href_pos) = sub.find("href=\"") {
                let href_start = href_pos + 6;
                if let Some(href_end) = sub[href_start..].find('"') {
                    let mut url = sub[href_start..href_start + href_end].to_string();
                    
                    if url.contains("uddg=") {
                        if let Some(uddg_pos) = url.find("uddg=") {
                            let uddg_param = &url[uddg_pos + 5..];
                            let amp_pos = uddg_param.find('&').unwrap_or(uddg_param.len());
                            let encoded = &uddg_param[..amp_pos];
                            if let Ok(decoded) = percent_encoding::percent_decode_str(encoded).decode_utf8() {
                                url = decoded.to_string();
                            }
                        }
                    }
                    
                    if let Some(close_tag_pos) = sub[href_start + href_end..].find('>') {
                        let title_start = href_start + href_end + close_tag_pos + 1;
                        if let Some(close_a_pos) = sub[title_start..].find("</a>") {
                            let raw_title = &sub[title_start..title_start + close_a_pos];
                            let title = clean_html_tags(raw_title);
                            
                            let mut snippet = String::new();
                            let snippet_class = "result__snippet";
                            if let Some(snippet_pos) = sub.find(snippet_class) {
                                let snip_sub = &sub[snippet_pos..];
                                if let Some(snip_open) = snip_sub.find('>') {
                                    let snip_start = snip_open + 1;
                                    if let Some(snip_close) = snip_sub[snip_start..].find("</a>") {
                                        let end_tags = vec!["</div>", "</p>", "</a>"];
                                        let mut min_close = snip_close;
                                        for tag in end_tags {
                                            if let Some(pos) = snip_sub[snip_start..].find(tag) {
                                                if pos < min_close {
                                                    min_close = pos;
                                                }
                                            }
                                        }
                                        let raw_snippet = &snip_sub[snip_start..snip_start + min_close];
                                        snippet = clean_html_tags(raw_snippet);
                                    }
                                }
                            }
                            
                            if !title.is_empty() && !url.is_empty() {
                                results.push(json!({
                                    "title": title,
                                    "url": url,
                                    "snippet": snippet
                                }));
                            }
                        }
                    }
                }
            }
        }
        if results.len() >= 5 {
            break;
        }
    }
    results
}

fn clean_html_tags(input: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for c in input.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            output.push(c);
        }
    }
    output = output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ");
    output.trim().to_string()
}

// =============================================================================
// DISPATCH MACRO HELPER
// =============================================================================

pub async fn execute_tools(
    session: &mut Session,
    response: &gemini_client_api::gemini::types::response::GeminiResponse,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    use gemini_client_api::execute_function_calls;
    
    // Check if there are function calls requested
    if response.get_chat().has_function_call() {
        execute_function_calls!(
            session,
            get_menu_config,
            set_menu_config,
            list_system_apps,
            google_search,
            ask_multiple_choice_question
        );
        Ok(true)
    } else {
        Ok(false)
    }
}
