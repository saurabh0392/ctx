use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use serde_json::{Map, Value};

use super::{PreservedField, ToolContract};

const MAX_CACHED_CONTRACT_BYTES: usize = 512 * 1024;
const MAX_DEFAULT_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// A lossless MCP `tools/list` result. The raw value is transient protocol evidence and is never
/// copied into [`McpToolContractCache`].
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMcpToolsList {
    pub tools: Vec<CanonicalMcpToolEntry>,
    pub next_cursor: PreservedField<String>,
    pub preserved: Map<String, Value>,
    raw: Value,
}

impl CanonicalMcpToolsList {
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn render(&self) -> Value {
        let mut result = self.preserved.clone();
        result.insert(
            "tools".into(),
            Value::Array(
                self.tools
                    .iter()
                    .map(CanonicalMcpToolEntry::render)
                    .collect(),
            ),
        );
        insert_preserved_field(&mut result, "nextCursor", &self.next_cursor);
        Value::Object(result)
    }
}

/// Malformed or future tool definitions remain opaque so a no-transform render cannot drop them.
#[derive(Debug, Clone, PartialEq)]
pub enum CanonicalMcpToolEntry {
    Tool(CanonicalMcpTool),
    Opaque(Value),
}

impl CanonicalMcpToolEntry {
    pub fn render(&self) -> Value {
        match self {
            Self::Tool(tool) => tool.render(),
            Self::Opaque(raw) => raw.clone(),
        }
    }

    pub fn tool(&self) -> Option<&CanonicalMcpTool> {
        match self {
            Self::Tool(tool) => Some(tool),
            Self::Opaque(_) => None,
        }
    }
}

/// One named MCP tool definition lifted out of `tools/list` without flattening its schemas or
/// extension fields.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMcpTool {
    pub name: String,
    pub contract: ToolContract,
    raw: Value,
}

impl CanonicalMcpTool {
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn render(&self) -> Value {
        let mut tool = self.contract.preserved.clone();
        tool.insert("name".into(), Value::String(self.name.clone()));
        insert_preserved_field(&mut tool, "inputSchema", &self.contract.input_schema);
        insert_preserved_field(&mut tool, "outputSchema", &self.contract.output_schema);
        insert_preserved_field(&mut tool, "annotations", &self.contract.annotations);
        Value::Object(tool)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolsListParseError {
    RootNotObject,
    MissingTools,
    ToolsNotArray,
}

impl fmt::Display for McpToolsListParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootNotObject => f.write_str("MCP tools/list result is not an object"),
            Self::MissingTools => f.write_str("MCP tools/list result has no tools field"),
            Self::ToolsNotArray => f.write_str("MCP tools/list tools field is not an array"),
        }
    }
}

impl std::error::Error for McpToolsListParseError {}

/// Parse the `result` object from an MCP `tools/list` response. A definition with no string name is
/// opaque; malformed schema fields on a named definition use [`PreservedField::Opaque`].
pub fn parse_mcp_tools_list(
    value: &Value,
) -> Result<CanonicalMcpToolsList, McpToolsListParseError> {
    let source = value
        .as_object()
        .ok_or(McpToolsListParseError::RootNotObject)?;
    let tools = source
        .get("tools")
        .ok_or(McpToolsListParseError::MissingTools)?
        .as_array()
        .ok_or(McpToolsListParseError::ToolsNotArray)?
        .iter()
        .map(parse_tool_entry)
        .collect();
    let next_cursor = match source.get("nextCursor") {
        None => PreservedField::Absent,
        Some(Value::String(cursor)) => PreservedField::Value(cursor.clone()),
        Some(value) => PreservedField::Opaque(value.clone()),
    };
    let preserved = source
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "tools" | "nextCursor"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    Ok(CanonicalMcpToolsList {
        tools,
        next_cursor,
        preserved,
        raw: value.clone(),
    })
}

fn parse_tool_entry(value: &Value) -> CanonicalMcpToolEntry {
    let Some(source) = value.as_object() else {
        return CanonicalMcpToolEntry::Opaque(value.clone());
    };
    let Some(name) = source.get("name").and_then(Value::as_str) else {
        return CanonicalMcpToolEntry::Opaque(value.clone());
    };
    if name.is_empty() {
        return CanonicalMcpToolEntry::Opaque(value.clone());
    }

    let contract = ToolContract {
        protocol_version: None,
        input_schema: object_field(source, "inputSchema"),
        output_schema: object_field(source, "outputSchema"),
        annotations: object_field(source, "annotations"),
        preserved: source
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "name" | "inputSchema" | "outputSchema" | "annotations"
                )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    };
    CanonicalMcpToolEntry::Tool(CanonicalMcpTool {
        name: name.into(),
        contract,
        raw: value.clone(),
    })
}

fn object_field(source: &Map<String, Value>, key: &str) -> PreservedField<Value> {
    match source.get(key) {
        None => PreservedField::Absent,
        Some(value) if value.is_object() => PreservedField::Value(value.clone()),
        Some(value) => PreservedField::Opaque(value.clone()),
    }
}

fn insert_preserved_field<T: Clone + Into<Value>>(
    target: &mut Map<String, Value>,
    key: &str,
    field: &PreservedField<T>,
) {
    match field {
        PreservedField::Absent => {}
        PreservedField::Value(value) => {
            target.insert(key.into(), value.clone().into());
        }
        PreservedField::Opaque(value) => {
            target.insert(key.into(), value.clone());
        }
    }
}

/// A contract identity cannot cross server or negotiated protocol boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McpToolContractKey {
    pub server_id: String,
    pub protocol_version: String,
    pub tool_name: String,
}

impl McpToolContractKey {
    pub fn new(server_id: &str, protocol_version: &str, tool_name: &str) -> Self {
        Self {
            server_id: server_id.into(),
            protocol_version: protocol_version.into(),
            tool_name: tool_name.into(),
        }
    }

    fn is_complete(&self) -> bool {
        !self.server_id.is_empty()
            && !self.protocol_version.is_empty()
            && !self.tool_name.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpContractCapture {
    pub captured: usize,
    pub opaque: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpContractCacheError {
    EmptyIdentity,
    DuplicateToolName(String),
    ContractTooLarge(String),
}

impl fmt::Display for McpContractCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentity => {
                f.write_str("MCP contract cache identity components must be non-empty")
            }
            Self::DuplicateToolName(name) => {
                write!(f, "MCP tools/list contains duplicate tool name {name:?}")
            }
            Self::ContractTooLarge(name) => {
                write!(f, "MCP tool contract {name:?} exceeds the cache size limit")
            }
        }
    }
}

impl std::error::Error for McpContractCacheError {}

/// A deterministic, bounded in-memory cache for schema-bearing tool contracts. It stores no raw
/// `tools/list` envelope, tool result, credential, description, icon, or vendor metadata.
#[derive(Debug, Clone)]
pub struct McpToolContractCache {
    max_entries: usize,
    max_total_bytes: usize,
    total_bytes: usize,
    entries: HashMap<McpToolContractKey, CachedToolContract>,
    insertion_order: VecDeque<McpToolContractKey>,
}

#[derive(Debug, Clone)]
struct CachedToolContract {
    contract: ToolContract,
    bytes: usize,
}

impl McpToolContractCache {
    pub fn new(max_entries: usize) -> Self {
        let max_entries = max_entries.max(1);
        let max_total_bytes = max_entries
            .saturating_mul(MAX_CACHED_CONTRACT_BYTES)
            .min(MAX_DEFAULT_CACHE_BYTES);
        Self::with_limits(max_entries, max_total_bytes)
    }

    pub fn with_limits(max_entries: usize, max_total_bytes: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            max_total_bytes: max_total_bytes.max(1),
            total_bytes: 0,
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &McpToolContractKey) -> Option<&ToolContract> {
        self.entries.get(key).map(|cached| &cached.contract)
    }

    pub fn insert(
        &mut self,
        key: McpToolContractKey,
        contract: &ToolContract,
    ) -> Result<(), McpContractCacheError> {
        if !key.is_complete() {
            return Err(McpContractCacheError::EmptyIdentity);
        }
        let bytes = contract_weight(contract);
        if bytes > MAX_CACHED_CONTRACT_BYTES || bytes > self.max_total_bytes {
            return Err(McpContractCacheError::ContractTooLarge(key.tool_name));
        }

        if let Some(existing) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(existing.bytes);
            self.insertion_order.retain(|existing| existing != &key);
        }
        while self.entries.len() >= self.max_entries
            || self.total_bytes.saturating_add(bytes) > self.max_total_bytes
        {
            if let Some(oldest) = self.insertion_order.pop_front() {
                if let Some(removed) = self.entries.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
                }
            } else {
                break;
            }
        }

        let cached = ToolContract {
            protocol_version: Some(key.protocol_version.clone()),
            input_schema: contract.input_schema.clone(),
            output_schema: contract.output_schema.clone(),
            annotations: contract.annotations.clone(),
            preserved: Map::new(),
        };
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.entries.insert(
            key.clone(),
            CachedToolContract {
                contract: cached,
                bytes,
            },
        );
        self.insertion_order.push_back(key);
        Ok(())
    }

    /// Capture one page only after proving that its named definitions are unique. A duplicate page
    /// fails atomically instead of leaving a partially refreshed contract set.
    pub fn capture_tools_list(
        &mut self,
        server_id: &str,
        protocol_version: &str,
        list: &CanonicalMcpToolsList,
    ) -> Result<McpContractCapture, McpContractCacheError> {
        if server_id.is_empty() || protocol_version.is_empty() {
            return Err(McpContractCacheError::EmptyIdentity);
        }
        let mut seen = HashSet::new();
        let mut tools = Vec::new();
        let mut opaque = 0;
        for entry in &list.tools {
            let Some(tool) = entry.tool() else {
                opaque += 1;
                continue;
            };
            if !seen.insert(tool.name.as_str()) {
                return Err(McpContractCacheError::DuplicateToolName(tool.name.clone()));
            }
            let bytes = contract_weight(&tool.contract);
            if bytes > MAX_CACHED_CONTRACT_BYTES || bytes > self.max_total_bytes {
                return Err(McpContractCacheError::ContractTooLarge(tool.name.clone()));
            }
            tools.push(tool);
        }

        for tool in &tools {
            self.insert(
                McpToolContractKey::new(server_id, protocol_version, &tool.name),
                &tool.contract,
            )?;
        }
        Ok(McpContractCapture {
            captured: tools.len(),
            opaque,
        })
    }

    pub fn invalidate_server(&mut self, server_id: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|key, cached| {
            if key.server_id == server_id {
                self.total_bytes = self.total_bytes.saturating_sub(cached.bytes);
                false
            } else {
                true
            }
        });
        self.insertion_order
            .retain(|key| key.server_id != server_id);
        before - self.entries.len()
    }
}

fn contract_weight(contract: &ToolContract) -> usize {
    32usize
        .saturating_add(preserved_field_weight(&contract.input_schema))
        .saturating_add(preserved_field_weight(&contract.output_schema))
        .saturating_add(preserved_field_weight(&contract.annotations))
}

fn preserved_field_weight(field: &PreservedField<Value>) -> usize {
    match field {
        PreservedField::Absent => 0,
        PreservedField::Value(value) | PreservedField::Opaque(value) => value_weight(value),
    }
}

fn value_weight(value: &Value) -> usize {
    let mut bytes = 0usize;
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        bytes = bytes.saturating_add(1);
        match value {
            Value::Null => bytes = bytes.saturating_add(4),
            Value::Bool(_) => bytes = bytes.saturating_add(5),
            Value::Number(number) => bytes = bytes.saturating_add(number.to_string().len()),
            Value::String(string) => bytes = bytes.saturating_add(string.len()),
            Value::Array(values) => stack.extend(values),
            Value::Object(object) => {
                for (key, value) in object {
                    bytes = bytes.saturating_add(key.len());
                    stack.push(value);
                }
            }
        }
        if bytes > MAX_CACHED_CONTRACT_BYTES {
            return bytes;
        }
    }
    bytes
}

impl Default for McpToolContractCache {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn list() -> CanonicalMcpToolsList {
        parse_mcp_tools_list(&json!({
            "tools": [{
                "name": "list_issues",
                "title": "List issues",
                "inputSchema": {"type": "object"},
                "outputSchema": {
                    "type": "object",
                    "properties": {"issues": {"type": "array"}},
                    "required": ["issues"]
                },
                "annotations": {"readOnlyHint": true},
                "_meta": {"vendor": true}
            }, 7],
            "nextCursor": "page-2",
            "vendor": true
        }))
        .expect("tools/list")
    }

    #[test]
    fn tools_list_round_trips_and_preserves_opaque_entries() {
        let list = list();
        assert_eq!(list.render(), *list.raw());
        assert_eq!(list.tools.len(), 2);
        assert!(matches!(list.tools[1], CanonicalMcpToolEntry::Opaque(_)));
        let tool = list.tools[0].tool().expect("known tool");
        assert_eq!(tool.render(), *tool.raw());
        assert!(matches!(
            tool.contract.output_schema,
            PreservedField::Value(_)
        ));
        assert_eq!(
            tool.contract.preserved.get("title"),
            Some(&json!("List issues"))
        );
    }

    #[test]
    fn malformed_optional_fields_stay_opaque() {
        let raw = json!({
            "tools": [{
                "name": "future_tool",
                "inputSchema": null,
                "outputSchema": "future-schema",
                "annotations": false
            }]
        });
        let list = parse_mcp_tools_list(&raw).expect("named tool");
        let tool = list.tools[0].tool().expect("known named tool");
        assert!(matches!(
            tool.contract.input_schema,
            PreservedField::Opaque(_)
        ));
        assert!(matches!(
            tool.contract.output_schema,
            PreservedField::Opaque(_)
        ));
        assert!(matches!(
            tool.contract.annotations,
            PreservedField::Opaque(_)
        ));
        assert_eq!(list.render(), raw);
    }

    #[test]
    fn cache_is_bounded_and_isolates_server_and_protocol_identities() {
        let list = list();
        let mut cache = McpToolContractCache::new(2);
        let captured = cache
            .capture_tools_list("linear", "2025-11-25", &list)
            .expect("capture");
        assert_eq!(
            captured,
            McpContractCapture {
                captured: 1,
                opaque: 1
            }
        );

        let linear = McpToolContractKey::new("linear", "2025-11-25", "list_issues");
        let cached = cache.get(&linear).expect("linear contract");
        assert_eq!(cached.protocol_version.as_deref(), Some("2025-11-25"));
        assert!(
            cached.preserved.is_empty(),
            "raw/vendor fields are not cached"
        );
        assert!(cache
            .get(&McpToolContractKey::new(
                "github",
                "2025-11-25",
                "list_issues"
            ))
            .is_none());
        assert!(cache
            .get(&McpToolContractKey::new(
                "linear",
                "2025-06-18",
                "list_issues"
            ))
            .is_none());

        cache
            .insert(
                McpToolContractKey::new("github", "2025-11-25", "list_issues"),
                &ToolContract::default(),
            )
            .expect("second");
        cache
            .insert(
                McpToolContractKey::new("filesystem", "2025-11-25", "read_file"),
                &ToolContract::default(),
            )
            .expect("third");
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&linear).is_none(), "oldest entry was evicted");
    }

    #[test]
    fn duplicate_tool_names_fail_without_partial_capture() {
        let duplicate = parse_mcp_tools_list(&json!({
            "tools": [
                {"name": "same", "inputSchema": {"type": "object"}},
                {"name": "same", "inputSchema": {"type": "object"}}
            ]
        }))
        .expect("list");
        let mut cache = McpToolContractCache::default();
        assert_eq!(
            cache.capture_tools_list("server", "2025-11-25", &duplicate),
            Err(McpContractCacheError::DuplicateToolName("same".into()))
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn oversized_contracts_fail_without_partial_capture() {
        let oversized = "x".repeat(MAX_CACHED_CONTRACT_BYTES + 1);
        let list = parse_mcp_tools_list(&json!({
            "tools": [
                {"name": "small", "inputSchema": {"type": "object"}},
                {
                    "name": "huge",
                    "inputSchema": {"type": "object"},
                    "outputSchema": {
                        "type": "object",
                        "description": oversized
                    }
                }
            ]
        }))
        .expect("list");
        let mut cache = McpToolContractCache::default();
        assert_eq!(
            cache.capture_tools_list("server", "2025-11-25", &list),
            Err(McpContractCacheError::ContractTooLarge("huge".into()))
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn invalidating_a_server_removes_all_of_its_protocol_versions() {
        let mut cache = McpToolContractCache::default();
        for protocol in ["2025-06-18", "2025-11-25"] {
            cache
                .insert(
                    McpToolContractKey::new("linear", protocol, "list_issues"),
                    &ToolContract::default(),
                )
                .expect("linear contract");
        }
        cache
            .insert(
                McpToolContractKey::new("github", "2025-11-25", "list_issues"),
                &ToolContract::default(),
            )
            .expect("github contract");
        assert_eq!(cache.invalidate_server("linear"), 2);
        assert_eq!(cache.len(), 1);
        assert!(cache
            .get(&McpToolContractKey::new(
                "github",
                "2025-11-25",
                "list_issues"
            ))
            .is_some());
    }
}
