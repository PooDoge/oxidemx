---
[flow]
id = "research-digest"
name = "Research Digest"
description = "Ingest a URL, digest it from two angles in parallel, and answer the user's question against the merged understanding."
version = 1

[inputs]
url = { type = "string", required = true }
question = { type = "string", default = "Summarize the key changes and why they matter." }

[defaults]
model = "gemini-2.5-flash"
executor = "react"
approval = "allowlist"
memory = "run"
max_turns = 8

[[step]]
id = "ingest"
agent = "web-researcher"
needs = []
task = "Fetch {{input.url}} and normalize the main content into clean markdown. Strip nav/ads/boilerplate; keep headings, code blocks, and links."
output = "debug/raw.md"
timeout_secs = 300
retry = { max = 2, backoff_secs = 5 }

[[step]]
id = "digest"
agent = "summarizer"
needs = ["ingest"]
task = "Digest the source into a tight brief. Keep this question in mind: {{input.question}}"
context = ["@artifact@"]
output = "debug/digest.md"
normalize = { title = "Source Digest", sections = ["Summary", "Key Points", "Open Questions"] }

[[step]]
id = "claims"
agent = "extractor"
needs = ["ingest"]
task = "Extract the concrete, checkable claims the source makes (facts, version numbers, dates, API/behaviour changes). One claim per bullet."
context = ["@artifact@"]
output = "debug/claims.md"

[[step]]
id = "stress-test"
kind = "reflect"
needs = ["digest"]
target = "digest"
critic = "skeptic"
max_rounds = 2
accept_when = "no_blocking_findings"
output = "debug/digest-reviewed.md"

[[step]]
id = "answer"
agent = "writer"
needs = ["stress-test", "claims"]
task = "Answer the user's question using the stress-tested digest and the extracted claims. Be specific and cite which claim each statement rests on. Question: {{input.question}}"
context = ["@step:stress-test@", "@step:claims@"]
output = "ANSWER.md"

[triggers]
slice = true
schedule = []
on_dbus = []

[delivery]
root = "ANSWER.md"
title = "Research digest"
note = "Final answer is ANSWER.md; the raw fetch, digest, and extracted claims are under debug/."
---

# Research Digest

Use this flow when the user wants a single source fetched, understood,
and interrogated against a question. It fans the understanding into two
parallel passes — a prose **digest** and a list of checkable **claims** —
then joins them so the final answer is both readable and grounded.

Good for: release notes, changelogs, RFCs, blog posts, single papers.
Not for: multi-source synthesis or open-web research (a `research-sweep`
flow is the right tool there).
