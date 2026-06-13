---
[flow]
id = "doc-digest"
name = "Document Digest"
description = "Parse a local document, digest it and extract its claims in parallel, then answer a question against the merged understanding."
version = 1

[inputs]
path = { type = "string", required = true }
question = { type = "string", default = "Summarize this document and its key points." }

[defaults]
model = "gemini-2.5-flash"
executor = "react"
approval = "allowlist"
max_turns = 8

[[step]]
id = "parse"
agent = "parser"
needs = []
task = "Load and normalize the document at {{input.path}} into clean markdown."
output = "debug/parsed.md"
timeout_secs = 180

[[step]]
id = "digest"
agent = "summarizer"
needs = ["parse"]
task = "Digest the document. Keep this question in mind: {{input.question}}"
context = ["@artifact@"]
output = "debug/digest.md"
normalize = { title = "Document Digest", sections = ["Summary", "Key Points"] }

[[step]]
id = "claims"
agent = "extractor"
needs = ["parse"]
task = "Extract the document's concrete, checkable claims (facts, figures, dates, decisions). One per bullet."
context = ["@artifact@"]
output = "debug/claims.md"

[[step]]
id = "answer"
agent = "writer"
needs = ["digest", "claims"]
task = "Answer the user's question using the digest and the extracted claims. Cite which claim each point rests on. Question: {{input.question}}"
context = ["@step:digest@", "@step:claims@"]
output = "ANSWER.md"

[triggers]
slice = true

[delivery]
root = "ANSWER.md"
title = "Document digest"
note = "Final answer is ANSWER.md; parsed text, digest, and claims under debug/."
---

# Document Digest

Use this flow to understand and interrogate a single local document —
a PDF, Word doc, spreadsheet, web page export, or markdown file. It
parses the file, then fans into a prose **digest** and a list of
checkable **claims**, and joins them into a grounded answer.

Good for: papers, reports, contracts, exported articles, spec docs.
For a remote URL instead of a local file, use `research-digest`.
