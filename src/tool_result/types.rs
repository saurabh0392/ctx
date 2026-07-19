use serde_json::{Map, Value};

/// One tool call and result after a platform adapter has lifted it out of its native wire shape.
///
/// T1/T2 populate this for lossless MCP fixtures and shadow contracts. Keeping the exchange here
/// prevents the native-hook and gateway migrations from inventing competing result models later.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalToolExchange<R = CanonicalMcpResult> {
    pub identity: ToolIdentity,
    pub transport: ToolTransport,
    pub input: Value,
    pub contract: ToolContract,
    pub result: R,
    pub provenance: ToolProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolIdentity {
    pub platform: String,
    pub server: Option<String>,
    pub tool: String,
    pub call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTransport {
    NativeHook,
    McpStdio,
    McpStreamableHttp,
    ShellWrapper,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolContract {
    pub protocol_version: Option<String>,
    pub input_schema: PreservedField<Value>,
    pub output_schema: PreservedField<Value>,
    pub annotations: PreservedField<Value>,
    pub preserved: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolProvenance {
    pub platform_version: Option<String>,
    pub os: Option<String>,
    pub adapter: Option<String>,
    pub observed_at: Option<String>,
    pub verification: Option<String>,
}

/// Distinguishes an absent optional field from a valid value and an extension/invalid value that
/// CTX does not understand. That distinction is required for a value-identical render.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PreservedField<T> {
    #[default]
    Absent,
    Value(T),
    Opaque(Value),
}

impl<T> PreservedField<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Absent | Self::Opaque(_) => None,
        }
    }

    pub fn is_present(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

/// A lossless MCP `CallToolResult` represented as typed content plus preserved extension fields.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMcpResult {
    pub content: Vec<CanonicalContentBlock>,
    pub structured_content: PreservedField<Value>,
    pub is_error: PreservedField<bool>,
    /// Every top-level field except `content`, `structuredContent`, and `isError`.
    pub preserved: Map<String, Value>,
    raw: Value,
}

impl CanonicalMcpResult {
    pub(crate) fn new(
        content: Vec<CanonicalContentBlock>,
        structured_content: PreservedField<Value>,
        is_error: PreservedField<bool>,
        preserved: Map<String, Value>,
        raw: Value,
    ) -> Self {
        Self {
            content,
            structured_content,
            is_error,
            preserved,
            raw,
        }
    }

    /// The exact parsed JSON value retained for fail-open recovery and identity checks.
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn is_error(&self) -> Option<bool> {
        self.is_error.value().copied()
    }

    pub fn metadata(&self) -> Option<&Value> {
        self.preserved.get("_meta")
    }

    /// Concatenates only text blocks. Non-text and opaque blocks remain represented separately and
    /// are never serialized into the candidate string.
    pub fn compressible_text(&self) -> Option<String> {
        let text: Vec<&str> = self
            .content
            .iter()
            .filter_map(CanonicalContentBlock::text)
            .collect();
        if text.is_empty() {
            None
        } else {
            Some(text.join("\n"))
        }
    }

    pub fn coverage(&self) -> McpResultCoverage {
        let mut coverage = McpResultCoverage {
            content_blocks: self.content.len(),
            has_structured_content: self.structured_content.is_present(),
            has_metadata: self.metadata().is_some(),
            opaque_contract_fields: usize::from(matches!(
                self.structured_content,
                PreservedField::Opaque(_)
            )) + usize::from(matches!(
                self.is_error,
                PreservedField::Opaque(_)
            )),
            ..Default::default()
        };
        for block in &self.content {
            match block {
                CanonicalContentBlock::Text { .. } => coverage.text_blocks += 1,
                CanonicalContentBlock::Image { .. } => coverage.image_blocks += 1,
                CanonicalContentBlock::Audio { .. } => coverage.audio_blocks += 1,
                CanonicalContentBlock::ResourceLink { .. } => coverage.resource_link_blocks += 1,
                CanonicalContentBlock::EmbeddedTextResource { .. } => {
                    coverage.embedded_text_resource_blocks += 1
                }
                CanonicalContentBlock::EmbeddedBlobResource { .. } => {
                    coverage.embedded_blob_resource_blocks += 1
                }
                CanonicalContentBlock::Unknown { .. } => coverage.unknown_blocks += 1,
            }
        }
        coverage
    }

    /// Rebuilds a protocol result from typed and preserved fields. No-transform renders are
    /// value-identical to `raw`; text transforms can later replace only a text field while every
    /// sibling block and extension remains intact.
    pub fn render(&self) -> Value {
        let mut result = self.preserved.clone();
        result.insert(
            "content".into(),
            Value::Array(
                self.content
                    .iter()
                    .map(CanonicalContentBlock::render)
                    .collect(),
            ),
        );
        insert_preserved_field(&mut result, "structuredContent", &self.structured_content);
        insert_preserved_field(&mut result, "isError", &self.is_error);
        Value::Object(result)
    }
}

fn insert_preserved_field<T: Clone + Into<Value>>(
    result: &mut Map<String, Value>,
    key: &str,
    field: &PreservedField<T>,
) {
    match field {
        PreservedField::Absent => {}
        PreservedField::Value(value) => {
            result.insert(key.to_string(), value.clone().into());
        }
        PreservedField::Opaque(value) => {
            result.insert(key.to_string(), value.clone());
        }
    }
}

/// Each understood block keeps its original map. Rendering overwrites only the typed fields, so
/// annotations, `_meta`, and vendor extensions survive even after a future text replacement.
#[derive(Debug, Clone, PartialEq)]
pub enum CanonicalContentBlock {
    Text {
        text: String,
        preserved: Map<String, Value>,
    },
    Image {
        data: String,
        mime_type: String,
        preserved: Map<String, Value>,
    },
    Audio {
        data: String,
        mime_type: String,
        preserved: Map<String, Value>,
    },
    ResourceLink {
        name: String,
        uri: String,
        preserved: Map<String, Value>,
    },
    EmbeddedTextResource {
        uri: String,
        text: String,
        preserved: Map<String, Value>,
    },
    EmbeddedBlobResource {
        uri: String,
        blob: String,
        preserved: Map<String, Value>,
    },
    Unknown {
        raw: Value,
    },
}

impl CanonicalContentBlock {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn render(&self) -> Value {
        match self {
            Self::Text { text, preserved } => {
                let mut block = preserved.clone();
                block.insert("type".into(), Value::String("text".into()));
                block.insert("text".into(), Value::String(text.clone()));
                Value::Object(block)
            }
            Self::Image {
                data,
                mime_type,
                preserved,
            } => render_binary_block("image", data, mime_type, preserved),
            Self::Audio {
                data,
                mime_type,
                preserved,
            } => render_binary_block("audio", data, mime_type, preserved),
            Self::ResourceLink {
                name,
                uri,
                preserved,
            } => {
                let mut block = preserved.clone();
                block.insert("type".into(), Value::String("resource_link".into()));
                block.insert("name".into(), Value::String(name.clone()));
                block.insert("uri".into(), Value::String(uri.clone()));
                Value::Object(block)
            }
            Self::EmbeddedTextResource {
                uri,
                text,
                preserved,
            } => render_embedded_resource(uri, "text", text, preserved),
            Self::EmbeddedBlobResource {
                uri,
                blob,
                preserved,
            } => render_embedded_resource(uri, "blob", blob, preserved),
            Self::Unknown { raw } => raw.clone(),
        }
    }
}

fn render_binary_block(
    block_type: &str,
    data: &str,
    mime_type: &str,
    preserved: &Map<String, Value>,
) -> Value {
    let mut block = preserved.clone();
    block.insert("type".into(), Value::String(block_type.into()));
    block.insert("data".into(), Value::String(data.into()));
    block.insert("mimeType".into(), Value::String(mime_type.into()));
    Value::Object(block)
}

fn render_embedded_resource(
    uri: &str,
    payload_key: &str,
    payload: &str,
    preserved: &Map<String, Value>,
) -> Value {
    let mut block = preserved.clone();
    block.insert("type".into(), Value::String("resource".into()));
    let mut resource = match block.remove("resource") {
        Some(Value::Object(resource)) => resource,
        Some(_) | None => Map::new(),
    };
    resource.insert("uri".into(), Value::String(uri.into()));
    resource.insert(payload_key.into(), Value::String(payload.into()));
    block.insert("resource".into(), Value::Object(resource));
    Value::Object(block)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct McpResultCoverage {
    pub content_blocks: usize,
    pub text_blocks: usize,
    pub image_blocks: usize,
    pub audio_blocks: usize,
    pub resource_link_blocks: usize,
    pub embedded_text_resource_blocks: usize,
    pub embedded_blob_resource_blocks: usize,
    pub unknown_blocks: usize,
    pub opaque_contract_fields: usize,
    pub has_structured_content: bool,
    pub has_metadata: bool,
}
