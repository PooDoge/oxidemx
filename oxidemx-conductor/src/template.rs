//! Prompt templating: `{{input.*}}` substitution and the kowalski
//! context tokens `@artifact@` / `@step:<id>@` (spec §8).
//!
//! Two distinct expansions, applied in order by the supervisor when it
//! assembles a step's concrete prompt:
//!
//! 1. **Input templating** — `{{input.<name>}}` and `{{<name>}}` are
//!    replaced from the validated run inputs. Closed-form: an
//!    unresolved `{{…}}` is an error (caught at validate time, §8).
//! 2. **Context tokens** — a step's `context = [...]` list may contain
//!    `@artifact@` (the single immediate predecessor's artifact) and
//!    `@step:<id>@` (a named ancestor's output). These expand to the
//!    upstream results the supervisor has collected, and are prepended
//!    to the task as labelled context blocks.

use std::collections::BTreeMap;

/// Error from input templating: a `{{token}}` with no matching input.
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("unresolved template token `{{{{{0}}}}}`")]
pub struct UnresolvedToken(pub String);

/// Expand `{{input.<name>}}` / `{{<name>}}` against the run inputs.
/// Whitespace inside the braces is tolerated (`{{ input.url }}`).
/// Returns an error naming the first unresolved token.
pub fn expand_inputs(
    template: &str,
    inputs: &BTreeMap<String, String>,
) -> Result<String, UnresolvedToken> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            // Unbalanced `{{` — emit verbatim and stop scanning.
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let raw = after[..end].trim();
        let key = raw.strip_prefix("input.").unwrap_or(raw);
        match inputs.get(key) {
            Some(v) => out.push_str(v),
            None => return Err(UnresolvedToken(raw.to_string())),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Find every `{{…}}` token in a template (the input key without the
/// `input.` prefix). Used by the validator to check templating is
/// closed against declared inputs before any run.
pub fn referenced_inputs(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let raw = after[..end].trim();
        let key = raw.strip_prefix("input.").unwrap_or(raw).to_string();
        if !found.contains(&key) {
            found.push(key);
        }
        rest = &after[end + 2..];
    }
    found
}

/// One upstream result the supervisor has collected for context
/// expansion: the producing step id and its output text.
#[derive(Debug, Clone)]
pub struct UpstreamResult {
    pub step_id: String,
    pub output: String,
}

/// Assemble a step's full prompt: labelled context blocks (from the
/// `context` tokens) followed by the input-expanded task.
///
/// `predecessor` is the single immediate predecessor for `@artifact@`
/// (the supervisor passes the sole `needs` entry's result; for a join
/// step `@artifact@` is ambiguous and validation forbids it — callers
/// use `@step:<id>@` there).
pub fn assemble_prompt(
    task: &str,
    context_tokens: &[String],
    predecessor: Option<&UpstreamResult>,
    by_step: &BTreeMap<String, String>,
) -> String {
    let mut blocks = String::new();
    for tok in context_tokens {
        let tok = tok.trim();
        if tok == "@artifact@" {
            if let Some(p) = predecessor {
                blocks.push_str(&format!(
                    "## Context — output of `{}`\n\n{}\n\n",
                    p.step_id, p.output
                ));
            }
        } else if let Some(id) = tok.strip_prefix("@step:").and_then(|s| s.strip_suffix('@')) {
            if let Some(out) = by_step.get(id) {
                blocks.push_str(&format!("## Context — output of `{id}`\n\n{out}\n\n"));
            }
        }
    }
    if blocks.is_empty() {
        task.to_string()
    } else {
        format!("{blocks}---\n\n# Task\n\n{task}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("url".to_string(), "https://example.com".to_string()),
            ("question".to_string(), "What changed?".to_string()),
        ])
    }

    #[test]
    fn expands_input_dot_and_bare_tokens_with_whitespace() {
        let got = expand_inputs("Fetch {{input.url}} re: {{ question }}", &inputs()).unwrap();
        assert_eq!(got, "Fetch https://example.com re: What changed?");
    }

    #[test]
    fn unresolved_token_is_an_error_naming_it() {
        let err = expand_inputs("Hi {{input.missing}}", &inputs()).unwrap_err();
        assert_eq!(err, UnresolvedToken("input.missing".to_string()));
    }

    #[test]
    fn referenced_inputs_lists_keys_without_prefix() {
        let refs = referenced_inputs("{{input.url}} and {{question}} and {{input.url}}");
        assert_eq!(refs, vec!["url".to_string(), "question".to_string()]);
    }

    #[test]
    fn assemble_prepends_artifact_and_named_step_context() {
        let pred = UpstreamResult {
            step_id: "ingest".into(),
            output: "RAW PAGE".into(),
        };
        let by_step = BTreeMap::from([("digest".to_string(), "DIGESTED".to_string())]);
        let got = assemble_prompt(
            "Write the answer.",
            &["@artifact@".into(), "@step:digest@".into()],
            Some(&pred),
            &by_step,
        );
        assert!(got.contains("output of `ingest`"));
        assert!(got.contains("RAW PAGE"));
        assert!(got.contains("output of `digest`"));
        assert!(got.contains("DIGESTED"));
        assert!(got.contains("# Task\n\nWrite the answer."));
    }

    #[test]
    fn no_context_returns_bare_task() {
        let got = assemble_prompt("just do it", &[], None, &BTreeMap::new());
        assert_eq!(got, "just do it");
    }
}
