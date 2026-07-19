use jsonschema::{Draft, PatternOptions};
use serde_json::Value;

use super::{CanonicalMcpResult, PreservedField, ToolContract};

const MAX_SCHEMA_NODES: usize = 20_000;
const MAX_SCHEMA_BYTES: usize = 512 * 1024;
const MAX_SCHEMA_DEPTH: usize = 64;
const MAX_INSTANCE_NODES: usize = 100_000;
const MAX_INSTANCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_INSTANCE_DEPTH: usize = 128;

/// Content-free reason a server-advertised output schema could not authorize a structured result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSchemaRejection {
    OutputSchemaNotObject,
    OutputSchemaRootNotObject,
    UnsupportedDialect,
    ExternalReference,
    SchemaTooLarge,
    SchemaTooDeep,
    InvalidSchema,
    StructuredContentMissing,
    StructuredContentOpaque,
    StructuredContentTooLarge,
    StructuredContentTooDeep,
    InstanceInvalid,
}

impl McpSchemaRejection {
    pub fn code(self) -> &'static str {
        match self {
            Self::OutputSchemaNotObject => "output-schema-not-object",
            Self::OutputSchemaRootNotObject => "output-schema-root-not-object",
            Self::UnsupportedDialect => "output-schema-unsupported-dialect",
            Self::ExternalReference => "output-schema-external-reference",
            Self::SchemaTooLarge => "output-schema-too-large",
            Self::SchemaTooDeep => "output-schema-too-deep",
            Self::InvalidSchema => "output-schema-invalid",
            Self::StructuredContentMissing => "structured-content-missing",
            Self::StructuredContentOpaque => "structured-content-not-object",
            Self::StructuredContentTooLarge => "structured-content-too-large",
            Self::StructuredContentTooDeep => "structured-content-too-deep",
            Self::InstanceInvalid => "structured-content-schema-mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpOutputSchemaValidation {
    NotAdvertised,
    Valid,
    Rejected(McpSchemaRejection),
}

impl McpOutputSchemaValidation {
    pub fn code(self) -> &'static str {
        match self {
            Self::NotAdvertised => "not-advertised",
            Self::Valid => "valid",
            Self::Rejected(rejection) => rejection.code(),
        }
    }

    pub fn advertised(self) -> bool {
        !matches!(self, Self::NotAdvertised)
    }
}

/// Validate a successful MCP result against the tool's advertised `outputSchema`. Schema and
/// instance traversal are bounded before compilation. Only same-document references are accepted;
/// CTX never resolves a server-provided schema through the network or filesystem.
pub fn validate_mcp_output_schema(
    contract: Option<&ToolContract>,
    result: &CanonicalMcpResult,
) -> McpOutputSchemaValidation {
    let Some(contract) = contract else {
        return McpOutputSchemaValidation::NotAdvertised;
    };
    let schema = match &contract.output_schema {
        PreservedField::Absent => return McpOutputSchemaValidation::NotAdvertised,
        PreservedField::Opaque(_) => {
            return McpOutputSchemaValidation::Rejected(McpSchemaRejection::OutputSchemaNotObject)
        }
        PreservedField::Value(schema) if schema.is_object() => schema,
        PreservedField::Value(_) => {
            return McpOutputSchemaValidation::Rejected(McpSchemaRejection::OutputSchemaNotObject)
        }
    };

    let Some(schema_object) = schema.as_object() else {
        return McpOutputSchemaValidation::Rejected(McpSchemaRejection::OutputSchemaNotObject);
    };
    if schema_object.get("type").and_then(Value::as_str) != Some("object") {
        return McpOutputSchemaValidation::Rejected(McpSchemaRejection::OutputSchemaRootNotObject);
    }
    if let Err(error) = check_bounds(schema, MAX_SCHEMA_NODES, MAX_SCHEMA_BYTES, MAX_SCHEMA_DEPTH) {
        return McpOutputSchemaValidation::Rejected(match error {
            BoundsError::TooLarge => McpSchemaRejection::SchemaTooLarge,
            BoundsError::TooDeep => McpSchemaRejection::SchemaTooDeep,
        });
    }
    if has_external_reference(schema) {
        return McpOutputSchemaValidation::Rejected(McpSchemaRejection::ExternalReference);
    }
    let draft = match schema_draft(schema) {
        Ok(draft) => draft,
        Err(rejection) => return McpOutputSchemaValidation::Rejected(rejection),
    };
    if !meta_schema_is_valid(draft, schema) {
        return McpOutputSchemaValidation::Rejected(McpSchemaRejection::InvalidSchema);
    }

    let structured = match &result.structured_content {
        PreservedField::Absent => {
            return McpOutputSchemaValidation::Rejected(
                McpSchemaRejection::StructuredContentMissing,
            )
        }
        PreservedField::Opaque(_) => {
            return McpOutputSchemaValidation::Rejected(McpSchemaRejection::StructuredContentOpaque)
        }
        PreservedField::Value(value) => value,
    };
    if let Err(error) = check_bounds(
        structured,
        MAX_INSTANCE_NODES,
        MAX_INSTANCE_BYTES,
        MAX_INSTANCE_DEPTH,
    ) {
        return McpOutputSchemaValidation::Rejected(match error {
            BoundsError::TooLarge => McpSchemaRejection::StructuredContentTooLarge,
            BoundsError::TooDeep => McpSchemaRejection::StructuredContentTooDeep,
        });
    }

    let validator = match jsonschema::options()
        .with_draft(draft)
        .with_pattern_options(PatternOptions::regex())
        .build(schema)
    {
        Ok(validator) => validator,
        Err(_) => return McpOutputSchemaValidation::Rejected(McpSchemaRejection::InvalidSchema),
    };
    if validator.is_valid(structured) {
        McpOutputSchemaValidation::Valid
    } else {
        McpOutputSchemaValidation::Rejected(McpSchemaRejection::InstanceInvalid)
    }
}

fn schema_draft(schema: &Value) -> Result<Draft, McpSchemaRejection> {
    let Some(declared) = schema.get("$schema") else {
        return Ok(Draft::Draft202012);
    };
    let Some(declared) = declared.as_str() else {
        return Err(McpSchemaRejection::InvalidSchema);
    };
    let declared = declared.trim_end_matches('#');
    match declared {
        "http://json-schema.org/draft-04/schema" | "https://json-schema.org/draft-04/schema" => {
            Ok(Draft::Draft4)
        }
        "http://json-schema.org/draft-06/schema" | "https://json-schema.org/draft-06/schema" => {
            Ok(Draft::Draft6)
        }
        "http://json-schema.org/draft-07/schema" | "https://json-schema.org/draft-07/schema" => {
            Ok(Draft::Draft7)
        }
        "http://json-schema.org/draft/2019-09/schema"
        | "https://json-schema.org/draft/2019-09/schema" => Ok(Draft::Draft201909),
        "http://json-schema.org/draft/2020-12/schema"
        | "https://json-schema.org/draft/2020-12/schema" => Ok(Draft::Draft202012),
        _ => Err(McpSchemaRejection::UnsupportedDialect),
    }
}

fn meta_schema_is_valid(draft: Draft, schema: &Value) -> bool {
    match draft {
        Draft::Draft4 => jsonschema::draft4::meta::is_valid(schema),
        Draft::Draft6 => jsonschema::draft6::meta::is_valid(schema),
        Draft::Draft7 => jsonschema::draft7::meta::is_valid(schema),
        Draft::Draft201909 => jsonschema::draft201909::meta::is_valid(schema),
        Draft::Draft202012 => jsonschema::draft202012::meta::is_valid(schema),
        _ => false,
    }
}

fn has_external_reference(value: &Value) -> bool {
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef")
                        && value
                            .as_str()
                            .is_some_and(|reference| !reference.starts_with('#'))
                    {
                        return true;
                    }
                    stack.push(value);
                }
            }
            Value::Array(values) => stack.extend(values),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundsError {
    TooLarge,
    TooDeep,
}

fn check_bounds(
    value: &Value,
    max_nodes: usize,
    max_bytes: usize,
    max_depth: usize,
) -> Result<(), BoundsError> {
    let mut nodes = 0usize;
    let mut bytes = 0usize;
    let mut stack = vec![(value, 0usize)];
    while let Some((value, depth)) = stack.pop() {
        if depth > max_depth {
            return Err(BoundsError::TooDeep);
        }
        nodes = nodes.saturating_add(1);
        if nodes > max_nodes {
            return Err(BoundsError::TooLarge);
        }
        match value {
            Value::Null => bytes = bytes.saturating_add(4),
            Value::Bool(_) => bytes = bytes.saturating_add(5),
            Value::Number(number) => bytes = bytes.saturating_add(number.to_string().len()),
            Value::String(string) => bytes = bytes.saturating_add(string.len()),
            Value::Array(values) => {
                bytes = bytes.saturating_add(values.len());
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(object) => {
                bytes = bytes.saturating_add(object.len());
                for (key, value) in object {
                    bytes = bytes.saturating_add(key.len());
                    stack.push((value, depth + 1));
                }
            }
        }
        if bytes > max_bytes {
            return Err(BoundsError::TooLarge);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;
    use crate::tool_result::parse_mcp_result;

    fn contract(schema: Value) -> ToolContract {
        ToolContract {
            output_schema: PreservedField::Value(schema),
            ..Default::default()
        }
    }

    fn result(structured: Value) -> CanonicalMcpResult {
        parse_mcp_result(&json!({
            "content": [{"type": "text", "text": structured.to_string()}],
            "structuredContent": structured
        }))
        .expect("result")
    }

    #[test]
    fn validates_default_2020_12_required_fields_and_types() {
        let contract = contract(json!({
            "type": "object",
            "properties": {
                "issues": {"type": "array", "items": {"type": "string"}},
                "nextCursor": {"type": ["string", "null"]}
            },
            "required": ["issues"],
            "additionalProperties": false
        }));
        assert_eq!(
            validate_mcp_output_schema(
                Some(&contract),
                &result(json!({"issues": ["CTX-1"], "nextCursor": null}))
            ),
            McpOutputSchemaValidation::Valid
        );
        assert_eq!(
            validate_mcp_output_schema(
                Some(&contract),
                &result(json!({"issues": [7], "extra": true}))
            ),
            McpOutputSchemaValidation::Rejected(McpSchemaRejection::InstanceInvalid)
        );
    }

    #[test]
    fn supports_local_references_and_explicit_draft_7() {
        let contract = contract(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "definitions": {
                "issue": {
                    "type": "object",
                    "properties": {"id": {"type": "string"}},
                    "required": ["id"]
                }
            },
            "properties": {
                "issue": {"$ref": "#/definitions/issue"}
            },
            "required": ["issue"]
        }));
        assert_eq!(
            validate_mcp_output_schema(
                Some(&contract),
                &result(json!({"issue": {"id": "CTX-75"}}))
            ),
            McpOutputSchemaValidation::Valid
        );
    }

    #[test]
    fn rejects_external_references_without_resolving_them() {
        let contract = contract(json!({
            "type": "object",
            "properties": {"issue": {"$ref": "https://example.com/issue.json"}}
        }));
        assert_eq!(
            validate_mcp_output_schema(Some(&contract), &result(json!({"issue": {}}))),
            McpOutputSchemaValidation::Rejected(McpSchemaRejection::ExternalReference)
        );
    }

    #[test]
    fn missing_malformed_and_schema_invalid_structured_content_fail_closed() {
        let contract = contract(json!({
            "type": "object",
            "properties": {"count": {"type": "integer"}},
            "required": ["count"]
        }));
        let missing = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "missing"}]
        }))
        .expect("missing structured content");
        assert_eq!(
            validate_mcp_output_schema(Some(&contract), &missing),
            McpOutputSchemaValidation::Rejected(McpSchemaRejection::StructuredContentMissing)
        );

        let opaque = parse_mcp_result(&json!({
            "content": [{"type": "text", "text": "opaque"}],
            "structuredContent": [1, 2, 3]
        }))
        .expect("opaque structured content");
        assert_eq!(
            validate_mcp_output_schema(Some(&contract), &opaque),
            McpOutputSchemaValidation::Rejected(McpSchemaRejection::StructuredContentOpaque)
        );

        let malformed = ToolContract {
            output_schema: PreservedField::Opaque(json!("future")),
            ..Default::default()
        };
        assert_eq!(
            validate_mcp_output_schema(Some(&malformed), &result(json!({}))),
            McpOutputSchemaValidation::Rejected(McpSchemaRejection::OutputSchemaNotObject)
        );
    }

    #[test]
    fn hostile_deep_schemas_are_bounded_before_compilation() {
        let mut nested = json!({"type": "string"});
        for _ in 0..70 {
            nested = json!({"allOf": [nested]});
        }
        let mut schema = Map::new();
        schema.insert("type".into(), json!("object"));
        schema.insert("properties".into(), json!({"value": nested}));
        let contract = contract(Value::Object(schema));
        assert_eq!(
            validate_mcp_output_schema(Some(&contract), &result(json!({"value": "x"}))),
            McpOutputSchemaValidation::Rejected(McpSchemaRejection::SchemaTooDeep)
        );
    }
}
