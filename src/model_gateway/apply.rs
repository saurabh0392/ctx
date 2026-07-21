//! Narrow M3 request preparation. Only an explicit testing route may enter this module, and only
//! the synthetic contract or an evidence-authorized Shell test result can produce a replacement.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::ToolResult;
use crate::config::Config;
use crate::tool_result::{PreparedTextTrim, TextApplyRequest, TextPrepareOutcome};

use super::canonical::CanonicalModelExchange;
use super::correlate::CorrelationOutcome;
use super::json_patch::{patch_text_leaves, TextLeafReplacement};
use super::registry::{ModelRoute, ModelRouteMode};

pub(super) struct PreparedModelRequest {
    pub body: Vec<u8>,
    pub trims: Vec<PreparedTextTrim>,
    pub reasons: BTreeMap<String, usize>,
}

impl PreparedModelRequest {
    pub fn mutated(&self) -> bool {
        !self.trims.is_empty()
    }

    pub fn unchanged(body: &[u8]) -> Self {
        unchanged(body)
    }
}

struct Candidate {
    replacement: String,
    kind: String,
    strategy: String,
}

#[cfg(test)]
pub(super) fn prepare_request(
    route: &ModelRoute,
    body: &[u8],
    observation: &CorrelationOutcome,
    config: &Config,
) -> PreparedModelRequest {
    prepare_request_inner(route, body, observation, config, None)
}

pub(super) fn prepare_request_with_cancellation(
    route: &ModelRoute,
    body: &[u8],
    observation: &CorrelationOutcome,
    config: &Config,
    cancelled: &AtomicBool,
) -> PreparedModelRequest {
    prepare_request_inner(route, body, observation, config, Some(cancelled))
}

fn prepare_request_inner(
    route: &ModelRoute,
    body: &[u8],
    observation: &CorrelationOutcome,
    config: &Config,
    cancelled: Option<&AtomicBool>,
) -> PreparedModelRequest {
    if route.mode != ModelRouteMode::Testing {
        return unchanged(body);
    }

    let mut prepared = Vec::new();
    let mut reasons = BTreeMap::new();
    for exchange in &observation.exchanges {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return unchanged(body);
        }
        if exchange.result.text_leaves.len() != 1 {
            reason(&mut reasons, "multiple-text-leaves-held");
            continue;
        }
        if exchange.result.is_error == Some(true) {
            reason(&mut reasons, "error-result-held");
            continue;
        }
        let original = &exchange.result.text_leaves[0].text;
        let Some(candidate) = candidate(exchange, config, route.surface.as_str()) else {
            reason(&mut reasons, "testing-contract-not-authorized");
            continue;
        };
        let command_or_path =
            crate::surface::fingerprint_tool_input(&exchange.identity.tool, &exchange.input);
        let protocol_version = exchange
            .provenance
            .adapter
            .as_deref()
            .unwrap_or("unknown-protocol");
        let request = TextApplyRequest {
            surface: route.surface.as_str(),
            route_id: &route.id,
            protocol_version,
            tool_name: &exchange.identity.tool,
            session_id: None,
            command_or_path: &command_or_path,
            kind: &candidate.kind,
            strategy: &candidate.strategy,
            original,
            replacement: &candidate.replacement,
            authorized: true,
            transport_latency_ms: None,
        };
        match crate::tool_result::prepare_text_trim(&request) {
            TextPrepareOutcome::Ready(trim) => prepared.push((exchange, *trim)),
            TextPrepareOutcome::PassThrough { reason: why } => reason(&mut reasons, why),
        }
    }

    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return unchanged(body);
    }
    if prepared.is_empty() {
        return PreparedModelRequest {
            body: body.to_vec(),
            trims: Vec::new(),
            reasons,
        };
    }
    let replacements = prepared
        .iter()
        .map(|(exchange, trim)| TextLeafReplacement {
            path: &exchange.result.text_leaves[0].path,
            expected: &exchange.result.text_leaves[0].text,
            replacement: &trim.replacement,
        })
        .collect::<Vec<_>>();
    let patched = match patch_text_leaves(body, &replacements) {
        Ok(patched) => patched,
        Err(error) => {
            reason(&mut reasons, error.code());
            return PreparedModelRequest {
                body: body.to_vec(),
                trims: Vec::new(),
                reasons,
            };
        }
    };
    PreparedModelRequest {
        body: patched,
        trims: prepared.into_iter().map(|(_, trim)| trim).collect(),
        reasons,
    }
}

fn candidate(
    exchange: &CanonicalModelExchange,
    config: &Config,
    surface: &str,
) -> Option<Candidate> {
    let original = exchange.result.combined_text();
    if exchange.identity.tool == "ctx_synthetic_echo"
        && exchange
            .input
            .get("contract")
            .and_then(serde_json::Value::as_str)
            == Some("ctx-synthetic-v1")
    {
        return synthetic_candidate(&original);
    }
    if !exchange.identity.tool.eq_ignore_ascii_case("shell") {
        return None;
    }
    let tool_result = ToolResult {
        tool_name: exchange.identity.tool.clone(),
        tool_input: exchange.input.clone(),
        raw_output: original.clone(),
        canonical_mcp: exchange.result.canonical_mcp.clone(),
        session_id: None,
        cwd: String::new(),
        recent_intent_text: None,
    };
    let decision = crate::agent::decide_for_surface(config, &tool_result, surface);
    if !decision.apply || decision.kind_label != "test" {
        return None;
    }
    let compressed = crate::compress::compress_tool_output(
        &tool_result.tool_name,
        &tool_result.tool_input,
        &tool_result.raw_output,
        config,
        None,
        "",
        false,
    )?;
    Some(Candidate {
        replacement: compressed.text,
        kind: decision.kind_label,
        strategy: compressed.strategy,
    })
}

fn synthetic_candidate(original: &str) -> Option<Candidate> {
    let lines = original.lines().collect::<Vec<_>>();
    if lines.len() < 32 {
        return None;
    }
    let mut retained = lines[..4].to_vec();
    retained.push("... synthetic middle omitted ...");
    retained.extend_from_slice(&lines[lines.len() - 4..]);
    let replacement = retained.join("\n");
    Some(Candidate {
        replacement,
        kind: "synthetic".into(),
        strategy: "ctx-synthetic-v1".into(),
    })
}

fn unchanged(body: &[u8]) -> PreparedModelRequest {
    PreparedModelRequest {
        body: body.to_vec(),
        trims: Vec::new(),
        reasons: BTreeMap::new(),
    }
}

fn reason(reasons: &mut BTreeMap<String, usize>, reason: &str) {
    *reasons.entry(reason.to_string()).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::model_gateway::protocols;
    use crate::model_gateway::registry::{ModelRouteMode, ProviderTarget};
    use crate::model_gateway::route::{AuthenticationMode, WireProtocol};
    use crate::surface::SurfaceId;

    fn route(protocol: WireProtocol, mode: ModelRouteMode) -> ModelRoute {
        ModelRoute {
            id: format!("test-{}", protocol.as_str()),
            surface: SurfaceId::Codex,
            protocol,
            authentication: AuthenticationMode::ApiKey,
            upstream: ProviderTarget::OpenAi,
            listen_port: 8871,
            mode,
        }
    }

    fn synthetic_output() -> String {
        (0..100)
            .map(|index| format!("synthetic line {index}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn synthetic_body(protocol: WireProtocol, output: &str) -> Value {
        match protocol {
            WireProtocol::AnthropicMessages => json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"call","name":"ctx_synthetic_echo","input":{"contract":"ctx-synthetic-v1"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":output}]}
            ]}),
            WireProtocol::OpenAiResponses => json!({"input":[
                {"type":"function_call","call_id":"call","name":"ctx_synthetic_echo","arguments":"{\"contract\":\"ctx-synthetic-v1\"}"},
                {"type":"function_call_output","call_id":"call","output":output}
            ]}),
            WireProtocol::OpenAiChatCompletions => json!({"messages":[
                {"role":"assistant","tool_calls":[{"id":"call","type":"function","function":{"name":"ctx_synthetic_echo","arguments":"{\"contract\":\"ctx-synthetic-v1\"}"}}]},
                {"role":"tool","tool_call_id":"call","content":output}
            ]}),
            WireProtocol::Unknown => unreachable!(),
        }
    }

    fn shell_body(protocol: WireProtocol, output: &str) -> Value {
        match protocol {
            WireProtocol::AnthropicMessages => json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"call","name":"Shell","input":{"command":"cargo test --workspace"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":output}]}
            ]}),
            WireProtocol::OpenAiResponses => json!({"input":[
                {"type":"function_call","call_id":"call","name":"Shell","arguments":"{\"command\":\"cargo test --workspace\"}"},
                {"type":"function_call_output","call_id":"call","output":output}
            ]}),
            WireProtocol::OpenAiChatCompletions => json!({"messages":[
                {"role":"assistant","tool_calls":[{"id":"call","type":"function","function":{"name":"Shell","arguments":"{\"command\":\"cargo test --workspace\"}"}}]},
                {"role":"tool","tool_call_id":"call","content":output}
            ]}),
            WireProtocol::Unknown => unreachable!(),
        }
    }

    #[test]
    fn synthetic_contract_patches_each_protocol_and_keeps_exact_rewind() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CTX_HOME");
        std::env::set_var("CTX_HOME", temp.path());
        let output = synthetic_output();

        for protocol in [
            WireProtocol::AnthropicMessages,
            WireProtocol::OpenAiResponses,
            WireProtocol::OpenAiChatCompletions,
        ] {
            let body = serde_json::to_vec(&synthetic_body(protocol, &output)).unwrap();
            let observation = protocols::inspect(protocol, "synthetic", &body);
            let prepared = prepare_request(
                &route(protocol, ModelRouteMode::Testing),
                &body,
                &observation,
                &Config::default(),
            );
            assert!(prepared.mutated(), "{protocol:?}");
            assert_eq!(prepared.trims.len(), 1);
            let parsed: Value = serde_json::from_slice(&prepared.body).unwrap();
            assert!(parsed.to_string().contains("ctx trimmed this output"));
            let conn = crate::db::open_db().unwrap();
            let stored = crate::db::get_rewind(&conn, &prepared.trims[0].rewind_id).unwrap();
            assert_eq!(stored.original, output);
            let applied: i64 = conn
                .query_row("SELECT COUNT(*) FROM compress_decisions", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(applied, 0, "prepare alone cannot claim acceptance");
        }

        if let Some(value) = previous {
            std::env::set_var("CTX_HOME", value);
        } else {
            std::env::remove_var("CTX_HOME");
        }
    }

    #[test]
    fn shadow_mode_is_byte_identical_and_writes_no_rewind() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CTX_HOME");
        std::env::set_var("CTX_HOME", temp.path());
        let protocol = WireProtocol::OpenAiResponses;
        let body = serde_json::to_vec(&synthetic_body(protocol, &synthetic_output())).unwrap();
        let observation = protocols::inspect(protocol, "synthetic", &body);
        let prepared = prepare_request(
            &route(protocol, ModelRouteMode::Shadow),
            &body,
            &observation,
            &Config::default(),
        );
        assert!(!prepared.mutated());
        assert_eq!(prepared.body, body);
        assert!(!crate::config::db_path().exists());

        if let Some(value) = previous {
            std::env::set_var("CTX_HOME", value);
        } else {
            std::env::remove_var("CTX_HOME");
        }
    }

    #[test]
    fn explicit_trial_activates_only_the_low_risk_shell_test_contract_on_each_protocol() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CTX_HOME");
        std::env::set_var("CTX_HOME", temp.path());
        let output = (0..200)
            .map(|index| format!("test case_{index} ... ok"))
            .collect::<Vec<_>>()
            .join("\n");
        let cfg = Config {
            active_profile: Some("all".into()),
            compress_enabled: true,
            compress_shadow_enabled: true,
            compress_trial_tools: vec!["Shell".into()],
            compress_tools: vec!["Shell".into()],
            compress_target_chars: 320,
            compress_max_output_chars: 32 * 1024,
            compress_redact_secrets: true,
            compress_preserve_errors: true,
            compress_explore_rate: 0.0,
            compress_explore_read_rate: 0.0,
            ..Default::default()
        };

        for protocol in [
            WireProtocol::AnthropicMessages,
            WireProtocol::OpenAiResponses,
            WireProtocol::OpenAiChatCompletions,
        ] {
            let body = serde_json::to_vec(&shell_body(protocol, &output)).unwrap();
            let observation = protocols::inspect(protocol, "testing", &body);
            let prepared = prepare_request(
                &route(protocol, ModelRouteMode::Testing),
                &body,
                &observation,
                &cfg,
            );
            assert!(prepared.mutated(), "{protocol:?}");
            assert_eq!(prepared.trims.len(), 1);
        }

        if let Some(value) = previous {
            std::env::set_var("CTX_HOME", value);
        } else {
            std::env::remove_var("CTX_HOME");
        }
    }
}
