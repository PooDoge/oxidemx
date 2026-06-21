---
[flow]
id = "log-analyzer"
name = "Log Analyzer"
description = "Analyzes log files daily, extracts key information, and provides a summary."
version = 1

[inputs]
log_directory = { type="string", default="/var/log" }
log_pattern = { type="string", required=false, default="*.log" }

[defaults]
model = "gemini-1.5-flash"
approval = "allowlist"
max_turns = 8

[[step]]
id = "list-logs"
agent = "web-researcher"
task = "List all files matching the pattern {{input.log_pattern}} in the directory {{input.log_directory}}."
output = "debug/log_files.txt"

[[step]]
id = "read-parse-logs"
agent = "extractor"
needs = ["list-logs"]
task = """
For each log file path provided in the context, read its content and extract:
- Timestamps
- Log level (e.g., INFO, WARNING, ERROR, CRITICAL)
- Source/Process ID (if available)
- Message content
Focus on identifying and extracting errors, warnings, and any other significant events. Output the extracted structured data in JSON format, with each log entry as an object.
"""
context = ["@artifact@"]
output = "debug/parsed_logs.json"

[[step]]
id = "summarize-analysis"
agent = "summarizer"
needs = ["read-parse-logs"]
task = """
Summarize the extracted log data from the context.
Highlight:
- The total number of errors, warnings, and critical events.
- Any recurring patterns or frequent error messages.
- Potential issues or anomalies identified in the logs.
- Provide a concise overview of the system's health based on the logs.
"""
context = ["@artifact@"]
output = "ANSWER.md"

[delivery]
root = "ANSWER.md"
title = "Log Analysis Report"
---
