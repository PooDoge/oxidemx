//! AutoAgents ⇄ Gemini Interactions wire translation.
//!
//! The Interactions API is server-side stateful: context lives behind
//! `previous_interaction_id`, so a request carries only the NEWEST
//! turn — a prompt string, or `function_result` object(s) when the
//! last round demanded tools. Wire shapes verified against the
//! production client (`overlay-rs/src/ai_client.rs:274-288, 559-625`).

use autoagents::llm::chat::{ChatMessage, ChatRole, MessageType, Tool};
use serde_json::{json, Value};

/// AutoAgents `Tool` (OpenAI-nested `{type, function:{…}}`) → the
/// FLAT declaration Interactions expects:
/// `{type:"function", name, description, parameters}`.
pub fn tools_to_interactions(tools: &[Tool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "name": t.function.name,
                "description": t.function.description,
                "parameters": t.function.parameters,
            })
        })
        .collect()
}

/// Extract the newest turn as the request `input`, plus the joined
/// system instruction.
///
/// * Last message is `ToolResult` → one `function_result` object,
///   or an array of them when the round produced several calls. The
///   executor stores each tool's RESULT in `ToolCall.function
///   .arguments` (string) — forwarded as the result text.
/// * Otherwise → the last non-system message's content as a JSON
///   string (the proven prompt shape).
/// * All `System` messages join (in order, blank-line separated) into
///   the `system_instruction` field.
pub fn newest_input(messages: &[ChatMessage]) -> (Value, Option<String>) {
    let system: Vec<&str> = messages
        .iter()
        .filter(|m| matches!(m.role, ChatRole::System))
        .map(|m| m.content.as_str())
        .collect();
    let system = if system.is_empty() {
        None
    } else {
        Some(system.join("\n\n"))
    };

    let newest = messages
        .iter()
        .rev()
        .find(|m| !matches!(m.role, ChatRole::System));

    let input = match newest {
        Some(m) => match &m.message_type {
            MessageType::ToolResult(calls) => {
                let results: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "type": "function_result",
                            "call_id": c.id,
                            "name": c.function.name,
                            "result": [{ "type": "text", "text": c.function.arguments }],
                        })
                    })
                    .collect();
                if results.len() == 1 {
                    // Bare object is the shape proven in production;
                    // arrays are the (unverified) multi-call path.
                    results.into_iter().next().unwrap()
                } else {
                    Value::Array(results)
                }
            }
            _ => json!(m.content),
        },
        None => json!(""),
    };
    (input, system)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoagents::llm::chat::FunctionTool;
    use autoagents::llm::{FunctionCall, ToolCall};

    fn msg(role: ChatRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            message_type: MessageType::Text,
            content: content.into(),
        }
    }

    fn tool_result(calls: Vec<(&str, &str, &str)>) -> ChatMessage {
        ChatMessage {
            role: ChatRole::Tool,
            message_type: MessageType::ToolResult(
                calls
                    .into_iter()
                    .map(|(id, name, result)| ToolCall {
                        id: id.into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: name.into(),
                            arguments: result.into(),
                        },
                    })
                    .collect(),
            ),
            content: String::new(),
        }
    }

    #[test]
    fn tools_translate_to_flat_interactions_shape() {
        let tools = [Tool {
            tool_type: "function".into(),
            function: FunctionTool {
                name: "execute_command".into(),
                description: "Run a command".into(),
                parameters: json!({"type":"object","properties":{"command":{"type":"string"}}}),
            },
        }];
        let v = tools_to_interactions(&tools);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["type"], "function");
        assert_eq!(v[0]["name"], "execute_command");
        assert_eq!(v[0]["description"], "Run a command");
        assert_eq!(v[0]["parameters"]["type"], "object");
        // Flat: no nested "function" object.
        assert!(v[0].get("function").is_none());
    }

    #[test]
    fn newest_user_message_becomes_string_input() {
        let messages = [
            msg(ChatRole::System, "You are helpful."),
            msg(ChatRole::User, "hi"),
        ];
        let (input, system) = newest_input(&messages);
        assert_eq!(input, json!("hi"));
        assert_eq!(system.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn newest_wins_over_history() {
        let messages = [
            msg(ChatRole::User, "old question"),
            msg(ChatRole::Assistant, "old answer"),
            msg(ChatRole::User, "new question"),
        ];
        let (input, system) = newest_input(&messages);
        assert_eq!(input, json!("new question"));
        assert!(system.is_none());
    }

    #[test]
    fn single_tool_result_becomes_bare_function_result_object() {
        let messages = [
            msg(ChatRole::User, "do it"),
            tool_result(vec![("call_1", "execute_command", "{\"output\":\"ok\"}")]),
        ];
        let (input, _) = newest_input(&messages);
        assert_eq!(input["type"], "function_result");
        assert_eq!(input["call_id"], "call_1");
        assert_eq!(input["name"], "execute_command");
        assert_eq!(input["result"][0]["type"], "text");
        assert_eq!(input["result"][0]["text"], "{\"output\":\"ok\"}");
    }

    #[test]
    fn multiple_tool_results_become_array() {
        let messages = [tool_result(vec![
            ("c1", "tool_a", "r1"),
            ("c2", "tool_b", "r2"),
        ])];
        let (input, _) = newest_input(&messages);
        let arr = input.as_array().expect("array for multi-call round");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["call_id"], "c1");
        assert_eq!(arr[1]["call_id"], "c2");
    }

    #[test]
    fn multiple_system_messages_join() {
        let messages = [
            msg(ChatRole::System, "Persona."),
            msg(ChatRole::System, "Memory rules."),
            msg(ChatRole::User, "q"),
        ];
        let (_, system) = newest_input(&messages);
        assert_eq!(system.as_deref(), Some("Persona.\n\nMemory rules."));
    }
}
