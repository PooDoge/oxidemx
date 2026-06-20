//! Edge schema validation for step inputs and outputs.
//!
//! Before a consumer step is dispatched, the executor validates the assembled
//! `inputs` object (built from the `needs` steps' outputs) against the
//! consumer's `input_schema`, if one is present.  Validation is performed
//! using the `jsonschema` crate with default-features disabled (no reqwest /
//! rustls pulled in).
//!
//! # Fail-closed policy
//!
//! A malformed schema (one that cannot be compiled by `jsonschema`) is treated
//! as an error rather than silently allowed.  This is the conservative choice:
//! a broken schema almost certainly indicates a planning bug, and running a
//! step whose input contract is unknown could silently produce garbage output.

/// Validate `payload` against `schema`.
///
/// Uses `jsonschema::validator_for` to compile the schema once and then
/// `validator.iter_errors(payload)` to collect all violations.
///
/// # Errors
///
/// - Returns `Err` if the schema cannot be compiled (malformed schema).
/// - Returns `Err` with the joined violation messages if any validation errors
///   are found.
/// - Returns `Ok(())` if `payload` is valid according to `schema`.
pub fn validate_edge(
    payload: &serde_json::Value,
    schema: &serde_json::Value,
) -> Result<(), String> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|e| format!("invalid schema: {e}"))?;

    let errors: Vec<String> = validator
        .iter_errors(payload)
        .map(|e| e.to_string())
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_payload_returns_ok() {
        let schema = json!({ "type": "object", "properties": { "n": { "type": "integer" } }, "required": ["n"] });
        let payload = json!({ "n": 42 });
        assert!(validate_edge(&payload, &schema).is_ok());
    }

    #[test]
    fn invalid_payload_returns_err() {
        let schema = json!({ "type": "object", "properties": { "n": { "type": "integer" } }, "required": ["n"] });
        let payload = json!({ "n": "not-an-int" });
        let result = validate_edge(&payload, &schema);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(!msg.is_empty());
    }

    #[test]
    fn malformed_schema_returns_err() {
        // "type" must be a string or array, not a boolean — should fail to compile.
        let schema = json!({ "type": true });
        let payload = json!({});
        assert!(validate_edge(&payload, &schema).is_err());
    }
}
