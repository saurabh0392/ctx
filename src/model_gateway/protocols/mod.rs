//! Independent model wire-protocol packs. Dispatch is exact: a route invokes only its declared
//! adapter, so evidence from one dialect cannot activate another.

mod anthropic_messages;
mod openai_responses;

use super::correlate::{CorrelationOutcome, CoverageReason};
use super::route::WireProtocol;

pub(super) const MAX_MODEL_RESULT_CHARS: usize = 2 * 1024 * 1024;
pub(super) const MAX_PROTOCOL_ITEMS: usize = 4096;

pub(super) fn inspect(protocol: WireProtocol, platform: &str, body: &[u8]) -> CorrelationOutcome {
    match protocol {
        WireProtocol::AnthropicMessages => anthropic_messages::inspect(platform, body),
        WireProtocol::OpenAiResponses => openai_responses::inspect(platform, body),
        WireProtocol::OpenAiChatCompletions | WireProtocol::Unknown => {
            let mut outcome = CorrelationOutcome::default();
            outcome.reason(CoverageReason::ProtocolShapeMismatch);
            outcome
        }
    }
}
