---
[flow]
id = "rust-agent-self-diagnose"
name = "Rust Agent Self-Diagnoser"
description = "Analyzes the Rust agent codebase for common integration patterns and potential issues related to LLM/agent framework integration."
version = 1

[inputs]
codebase_path = { type="string", required=true, default="." }

[defaults]
model = "gemini-1.5-pro"
approval = "allowlist"
max_turns = 10

[[step]]
id = "list_rust_files"
agent = "web-researcher"
task = "List all Rust source files (.rs) recursively within the '{{input.codebase_path}}' directory."
output = "debug/rust_files.txt"

[[step]]
id = "analyze_codebase"
agent = "extractor"
needs = ["list_rust_files"]
task = """
Given the list of Rust files from the 'list_rust_files' step, identify files likely to contain LLM agent integration code (e.g., in `src/agents/`, `src/providers/`, `src/tools/`, or `src/main.rs`, `src/lib.rs`).

For these identified files, read their content using the `read_file` tool and analyze them for the following patterns, which are common integration points or potential pitfalls when building LLM agents in Rust:

1.  **`ChatProvider::chat_with_tools`**: Look for implementations of this trait method. Verify if it's the *only* required method implemented and if other provider trait methods are stubbed as discussed in the `building-llm-agents-in-rust` skill.
2.  **`ToolResult` handling**: Check how `ToolResult` messages are processed after a tool call. Ensure the tool's output within `ToolCall.function.arguments` is correctly translated to the API's function-result shape.
3.  **Server-side session APIs**: If relevant, look for logic related to managing a single `session ID` per conversation and sending only the newest message with `previous_interaction_id`, ignoring the executor's full history.
4.  **Stateful tools**: Identify if custom `AgentDeriveT` implementations are used for stateful tools that return `Vec<Box<dyn ToolT>>`.
5.  **Token streaming**: Look for code that handles streaming tokens to the UI, especially within the provider's HTTP round, forwarding SSE text deltas.
6.  **Mid-turn cancellation**: Search for `tokio::select!` with `CancellationToken` inside the provider's request future or tied into approval `.await`s.
7.  **Agent `description()` method**: Check if the `description()` method is built dynamically to inject persona and memory.
8.  **Blocking API responses**: Verify if code handles potential omissions of fields in blocking API responses compared to streaming paths.
9.  **Policy gating**: Look for allowlist checks inside `execute()` methods and logic for prompting approval dialogs for off-list commands, returning structured `Ok` results on denial.

Summarize any potential issues, deviations from best practices, or areas that could be improved based on these patterns. Prioritize critical findings.
"""
context = ["@step:list_rust_files@"]
output = "debug/analysis_report.md"

[[step]]
id = "generate_report"
agent = "writer"
needs = ["analyze_codebase"]
task = "Compile the findings from the 'analyze_codebase' step into a concise and actionable report, highlighting potential issues and recommended improvements for the Rust agent codebase. The report should be easy to understand for a developer."
context = ["@step:analyze_codebase@"]
output = "ANSWER.md"

[delivery]
root = "ANSWER.md"
title = "Rust Agent Codebase Self-Diagnosis Report"
---
