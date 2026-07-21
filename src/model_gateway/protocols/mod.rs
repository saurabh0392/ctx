//! Independent model wire-protocol packs. Dispatch is exact: a route invokes only its declared
//! adapter, so evidence from one dialect cannot activate another.

mod anthropic_messages;
mod openai_chat;
mod openai_responses;

use super::correlate::{CorrelationOutcome, CoverageReason};
use super::route::WireProtocol;

pub(super) const MAX_MODEL_RESULT_CHARS: usize = 2 * 1024 * 1024;
pub(super) const MAX_PROTOCOL_ITEMS: usize = 4096;

pub(super) fn inspect(protocol: WireProtocol, platform: &str, body: &[u8]) -> CorrelationOutcome {
    match protocol {
        WireProtocol::AnthropicMessages => anthropic_messages::inspect(platform, body),
        WireProtocol::OpenAiResponses => openai_responses::inspect(platform, body),
        WireProtocol::OpenAiChatCompletions => openai_chat::inspect(platform, body),
        WireProtocol::Unknown => {
            let mut outcome = CorrelationOutcome::default();
            outcome.reason(CoverageReason::ProtocolShapeMismatch);
            outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    fn equivalent_bodies(output: &str) -> [(WireProtocol, Value); 3] {
        [
            (
                WireProtocol::AnthropicMessages,
                json!({"messages":[
                    {"role":"assistant","content":[{"type":"tool_use","id":"call","name":"Shell","input":{"command":"cargo test --workspace"}}]},
                    {"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":output}]}
                ]}),
            ),
            (
                WireProtocol::OpenAiResponses,
                json!({"input":[
                    {"type":"function_call","call_id":"call","name":"Shell","arguments":"{\"command\":\"cargo test --workspace\"}"},
                    {"type":"function_call_output","call_id":"call","output":output}
                ]}),
            ),
            (
                WireProtocol::OpenAiChatCompletions,
                json!({"messages":[
                    {"role":"assistant","tool_calls":[{"id":"call","type":"function","function":{"name":"Shell","arguments":"{\"command\":\"cargo test --workspace\"}"}}]},
                    {"role":"tool","tool_call_id":"call","content":output}
                ]}),
            ),
        ]
    }

    #[test]
    fn equivalent_protocol_fixtures_produce_the_same_canonical_strategy_decision() {
        let output = (0..80)
            .map(|index| format!("test case {index} passed"))
            .collect::<Vec<_>>()
            .join("\n");
        let cfg = crate::config::Config {
            active_profile: Some("all".into()),
            compress_target_chars: 160,
            compress_max_output_chars: 16 * 1024,
            compress_redact_secrets: true,
            compress_preserve_errors: true,
            ..Default::default()
        };
        let mut canonical = Vec::new();
        let mut decisions = Vec::new();
        for (protocol, body) in equivalent_bodies(&output) {
            let observed = inspect(protocol, "equivalent-surface", &bytes(&body));
            assert!(observed.reasons.is_empty(), "{protocol:?}");
            assert_eq!(observed.exchanges.len(), 1, "{protocol:?}");
            let exchange = &observed.exchanges[0];
            canonical.push((
                exchange.identity.tool.clone(),
                exchange.input.clone(),
                exchange.result.combined_text(),
            ));
            let decision = crate::compress::compute_shadow_decision_with_mcp_contract(
                &exchange.identity.tool,
                &exchange.input,
                &exchange.result.combined_text(),
                exchange.result.canonical_mcp.as_ref(),
                Some(&exchange.contract),
                &cfg,
                None,
                "",
            )
            .unwrap();
            decisions.push((
                decision.kind,
                decision.lines_total,
                decision.lines_keep,
                decision.lines_drop,
                decision.chars_in,
                decision.would_chars_out,
            ));
        }
        assert!(canonical.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(decisions.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn no_protocol_adapter_can_activate_another_protocol_body() {
        let bodies = equivalent_bodies("secret output");
        for (selected_protocol, _) in &bodies {
            for (body_protocol, body) in &bodies {
                let observed = inspect(*selected_protocol, "isolated", &bytes(body));
                if selected_protocol == body_protocol {
                    assert_eq!(observed.exchanges.len(), 1);
                } else {
                    assert!(
                        observed.exchanges.is_empty(),
                        "{selected_protocol:?} accepted {body_protocol:?}"
                    );
                }
            }
        }
    }
}
