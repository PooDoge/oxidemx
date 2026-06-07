use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use std::sync::Mutex;
use once_cell::sync::Lazy;
use tracing::{info, warn, error};

// =============================================================================
// GLOBAL CHANNELS FOR ASYNC TOOL-TO-UI COMMUNICATION
// =============================================================================

#[derive(Debug, Clone)]
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
// REST REQUEST/RESPONSE DATA MODELS FOR THE INTERACTIONS API
// =============================================================================

#[derive(Serialize, Clone, Debug)]
pub struct CreateInteractionRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,
    pub input: InteractionInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum InteractionInput {
    Text(String),
    FunctionResult(FunctionResultInput),
}

#[derive(Serialize, Clone, Debug)]
pub struct FunctionResultInput {
    pub r#type: String, // Always "function_result"
    pub call_id: String,
    pub name: String,
    pub result: Vec<FunctionResultBlock>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FunctionResultBlock {
    pub r#type: String, // Always "text"
    pub text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct InteractionResponse {
    pub id: String,
    pub status: String, // "completed", "requires_action", etc.
    pub steps: Vec<InteractionStep>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct InteractionStep {
    pub r#type: String, // "user_input", "thought", "model_output", "function_call", "function_result"
    pub status: String, // "done", "waiting"
    #[serde(default)]
    pub id: Option<String>, // Present on "function_call" (matches call_id)
    #[serde(default)]
    pub name: Option<String>, // Present on "function_call" / "function_result"
    #[serde(default)]
    pub arguments: Option<serde_json::Value>, // JSON arguments present on "function_call"
    #[serde(default)]
    pub content: Option<Vec<StepContent>>, // Present on text/thought steps
}

#[derive(Deserialize, Debug, Clone)]
pub struct StepContent {
    pub r#type: String, // "text"
    pub text: String,
}

// =============================================================================
// API KEY & CONFIG PATH RESOLVERS
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
// AGENT MODES & PROMPT DEFS
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    GeneralChat,
    SettingsCustomizer,
}

impl AgentMode {
    pub fn system_instruction(&self) -> &'static str {
        match self {
            AgentMode::GeneralChat => {
                "You are OxideMX-AI, a helpful conversational desktop assistant. \
                 You can answer questions, explain concepts, and query the web to ground your responses in real-time. \
                 Keep your responses concise, user-friendly, and format them in markdown."
            }
            AgentMode::SettingsCustomizer => {
                "You are the OxideMX Settings Customizer. You specialize in configuring \
                 the OxideMX circular radial menu overlay, mouse remapping shortcuts, visual themes, and animation curves. \
                 You can read and modify the active layout config. When generating themes, layouts, or list recommendations, \
                 you can output structures in JSON matching the specified schemas. \
                 If you need to make changes, call the set_menu_config tool. \
                 If you have questions with multiple choice options, call the ask_multiple_choice_question tool. \
                 Keep your text replies clean, direct, and focused on layout modification."
            }
        }
    }

    pub fn tools(&self) -> Vec<serde_json::Value> {
        let search_tool = json!({
            "type": "function",
            "name": "google_search",
            "description": "Search the web for real-time information using DuckDuckGo. Returns a list of titles, links, and snippets.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query terms"
                    }
                },
                "required": ["query"]
            }
        });

        match self {
            AgentMode::GeneralChat => vec![search_tool],
            AgentMode::SettingsCustomizer => vec![
                json!({
                    "type": "function",
                    "name": "get_menu_config",
                    "description": "Retrieve the current OxideMX radial menu layout, animation curves, and mouse button configuration.",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "type": "function",
                    "name": "set_menu_config",
                    "description": "Overwrite the current OxideMX radial menu configuration with a new JSON setup. Use this to save changes to themes, layout slices, custom pages, or animation speeds.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "config_json": {
                                "type": "string",
                                "description": "The complete new configuration JSON string"
                            }
                        },
                        "required": ["config_json"]
                    }
                }),
                json!({
                    "type": "function",
                    "name": "list_system_apps",
                    "description": "Scan the host system's desktop directories to list installed applications, commands, and icons. Helpful for recommending executables to bind to custom slices.",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                search_tool,
                json!({
                    "type": "function",
                    "name": "ask_multiple_choice_question",
                    "description": "Ask the user a clarifying multiple-choice question. Used when there are multiple valid options or parameters to clarify.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The question text to present"
                            },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "string"
                                },
                                "description": "The list of choices/options the user can click"
                            }
                        },
                        "required": ["question", "options"]
                    }
                })
            ]
        }
    }
}

// =============================================================================
// MAIN ASYNC API CLIENT FUNCTION (AGENT LOOP)
// =============================================================================

pub async fn ask_ai(
    api_key: &str,
    mode: AgentMode,
    prompt: &str,
    mut session_id: Option<String>,
) -> Result<(String, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let model = "models/gemini-2.5-flash";
    let tools = mode.tools();
    let sys_prompt = mode.system_instruction();

    // Start with the initial user prompt as text input
    let mut current_input = InteractionInput::Text(prompt.to_string());

    loop {
        // Construct the request payload
        let req_body = CreateInteractionRequest {
            model: model.to_string(),
            previous_interaction_id: session_id.clone(),
            input: current_input,
            tools: Some(tools.clone()),
            system_instruction: Some(sys_prompt.to_string()),
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta2/interactions?key={}",
            api_key
        );

        info!("Sending request to Interactions API. session_id={:?}", session_id);
        let res = client
            .post(&url)
            .json(&req_body)
            .send()
            .await?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            error!("Interactions API returned error: {}", error_text);
            return Err(format!("API error: {}", error_text).into());
        }

        let resp_body: InteractionResponse = res.json().await?;
        info!("Received response: status='{}', id='{}'", resp_body.status, resp_body.id);

        // Update the session ID for subsequent turns (if any)
        session_id = Some(resp_body.id.clone());

        match resp_body.status.as_str() {
            "completed" => {
                // Collect all text from model outputs
                let mut accumulated_text = String::new();
                for step in resp_body.steps {
                    if step.r#type == "model_output" {
                        if let Some(contents) = step.content {
                            for content in contents {
                                if content.r#type == "text" {
                                    if !accumulated_text.is_empty() {
                                        accumulated_text.push('\n');
                                    }
                                    accumulated_text.push_str(&content.text);
                                }
                            }
                        }
                    }
                }
                return Ok((accumulated_text, session_id));
            }
            "requires_action" => {
                // Find the waiting function call
                let mut found_call = None;
                for step in &resp_body.steps {
                    if step.r#type == "function_call" && step.status == "waiting" {
                        if let (Some(id), Some(name), Some(args)) = (&step.id, &step.name, &step.arguments) {
                            found_call = Some((id.clone(), name.clone(), args.clone()));
                            break;
                        }
                    }
                }

                if let Some((call_id, name, args)) = found_call {
                    info!("Executing local tool '{}' (call_id={})", name, call_id);
                    let result_text = execute_local_tool(&name, args).await?;
                    
                    // Set up the next request's input to feed this function result back to the model
                    current_input = InteractionInput::FunctionResult(FunctionResultInput {
                        r#type: "function_result".to_string(),
                        call_id,
                        name,
                        result: vec![FunctionResultBlock {
                            r#type: "text".to_string(),
                            text: result_text,
                        }],
                    });

                    // Continue loop to submit function result
                    continue;
                } else {
                    warn!("Interactions status was 'requires_action' but no waiting function call was found!");
                    return Err("requires_action status with no pending function call".into());
                }
            }
            other => {
                error!("Unrecognized interaction status: '{}'", other);
                return Err(format!("Unrecognized interaction status: {}", other).into());
            }
        }
    }
}

// =============================================================================
// LOCAL TOOL EXECUTION ROUTER
// =============================================================================

async fn execute_local_tool(
    name: &str,
    args: serde_json::Value,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match name {
        "get_menu_config" => {
            let path = get_config_path();
            if !path.exists() {
                let default_bytes = include_str!("../../oxidemx-shared/default-config.json");
                return Ok(default_bytes.to_string());
            }
            let content = tokio::fs::read_to_string(&path).await?;
            Ok(content)
        }
        "set_menu_config" => {
            let config_json = args["config_json"]
                .as_str()
                .ok_or("config_json argument missing or not a string")?;
            
            // Validate JSON format
            let _: serde_json::Value = serde_json::from_str(config_json)?;
            let path = get_config_path();
            
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            
            // Notify UI
            {
                let tx_opt = CONFIG_CHANGED_TX.lock().unwrap().clone();
                if let Some(tx) = tx_opt {
                    let _ = tx.send(config_json.to_string()).await;
                }
            }
            
            tokio::fs::write(&path, config_json).await?;
            Ok("Configuration saved successfully".to_string())
        }
        "list_system_apps" => {
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
        "google_search" => {
            let query = args["query"]
                .as_str()
                .ok_or("query argument missing or not a string")?;
            
            let client = reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .timeout(std::time::Duration::from_secs(8))
                .build()?;
            
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
        "ask_multiple_choice_question" => {
            let question = args["question"]
                .as_str()
                .ok_or("question argument missing or not a string")?;
            let options_val = args["options"]
                .as_array()
                .ok_or("options argument missing or not an array")?;
            
            let mut options = Vec::new();
            for opt in options_val {
                if let Some(opt_str) = opt.as_str() {
                    options.push(opt_str.to_string());
                }
            }
            
            let tx_opt = QUESTION_TX.lock().unwrap().clone();
            if let Some(tx) = tx_opt {
                let (resp_tx, mut resp_rx) = mpsc::channel(1);
                let pending = PendingQuestion {
                    question: question.to_string(),
                    options,
                    response_tx: resp_tx,
                };
                tx.send(pending).await?;
                
                if let Some(answer) = resp_rx.recv().await {
                    Ok(answer)
                } else {
                    Err("Response channel closed".into())
                }
            } else {
                Err("Question channel not initialized".into())
            }
        }
        other => Err(format!("Unknown tool: {}", other).into()),
    }
}

// =============================================================================
// DDG HTML PARSER HELPERS
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
