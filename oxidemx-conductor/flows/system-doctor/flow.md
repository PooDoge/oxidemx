---
[flow]
id = "system-doctor"
name = "System Doctor"
description = "Gather read-only system diagnostics, stress-test the findings for accuracy, and write a health report."
version = 1

[inputs]
focus = { type = "string", default = "overall desktop health: disk, memory, failed services, recent errors, pending updates" }

[defaults]
model = "gemini-2.5-flash"
executor = "react"
approval = "allowlist"
max_turns = 10

[[step]]
id = "diagnose"
agent = "sysadmin"
needs = []
task = "Run read-only diagnostics and report the facts. Focus: {{input.focus}}"
output = "debug/diagnostics.md"
timeout_secs = 240

[[step]]
id = "review"
kind = "reflect"
needs = ["diagnose"]
target = "diagnose"
critic = "skeptic"
max_rounds = 2
accept_when = "no_blocking_findings"
output = "debug/diagnostics-reviewed.md"

[[step]]
id = "report"
agent = "writer"
needs = ["review"]
task = "Write a concise system health report from the reviewed diagnostics: what's healthy, what needs attention, and any recommended next steps (read-only — suggest, don't execute)."
context = ["@step:review@"]
output = "REPORT.md"

[triggers]
slice = true
schedule = []

[delivery]
root = "REPORT.md"
title = "System health report"
note = "REPORT.md is the summary; raw diagnostics under debug/."
---

# System Doctor

A read-only desktop health check. The `sysadmin` agent gathers facts
with `execute_command` (uptime, disk, memory, failed services, recent
errors, `rpm-ostree status`), a `skeptic` reflect pass stress-tests the
findings, and the `writer` turns them into a report with recommended
next steps.

Diagnostics only — it never changes system state. The `execute_command`
calls are gated by your command allowlist (Settings → AI), so allowlist
the read-only commands you're comfortable with (df, free, systemctl,
journalctl, uptime, rpm-ostree) for the fullest report.
