//! Content-local shadow execution. Requests are inspected in bounded memory, forwarded unchanged,
//! and reduced to content-free counters before the request stack frame is dropped.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::http::header::CONTENT_ENCODING;
use axum::http::HeaderMap;
use serde::Serialize;

use crate::config::Config;

use super::correlate::{CorrelationOutcome, CoverageReason};
use super::protocols;
use super::registry::ModelRoute;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShadowHealthReceipt {
    pub mode: &'static str,
    pub requests_observed: u64,
    pub exchanges_correlated: u64,
    pub decisions_computed: u64,
    pub would_shorten: u64,
    pub last_coverage_reasons: BTreeMap<String, usize>,
    pub raw_requests_persisted: bool,
}

#[derive(Debug, Default)]
struct ShadowStats {
    requests_observed: u64,
    exchanges_correlated: u64,
    decisions_computed: u64,
    would_shorten: u64,
    last_coverage_reasons: BTreeMap<String, usize>,
}

#[derive(Clone)]
pub(super) struct ShadowEngine {
    config: Arc<Config>,
    stats: Arc<Mutex<ShadowStats>>,
}

impl ShadowEngine {
    pub(super) fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            stats: Arc::new(Mutex::new(ShadowStats::default())),
        }
    }

    pub(super) fn observe(&self, route: &ModelRoute, headers: &HeaderMap, body: &[u8]) {
        let mut outcome = if identity_encoded(headers) {
            protocols::inspect(route.protocol, route.surface.as_str(), body)
        } else {
            let mut outcome = CorrelationOutcome::default();
            outcome.reason(CoverageReason::UnsupportedContentEncoding);
            outcome
        };
        let exchanges = outcome.exchanges.len() as u64;
        let mut decisions = 0u64;
        let mut would_shorten = 0u64;

        let correlated = std::mem::take(&mut outcome.exchanges);
        for exchange in correlated {
            if exchange.result.already_shortened {
                outcome.reason(CoverageReason::AlreadyShortened);
                continue;
            }
            let raw_output = exchange.result.combined_text();
            let decision = crate::compress::compute_shadow_decision_with_mcp_contract(
                &exchange.identity.tool,
                &exchange.input,
                &raw_output,
                exchange.result.canonical_mcp.as_ref(),
                Some(&exchange.contract),
                &self.config,
                None,
                "",
            );
            let Some(decision) = decision else {
                outcome.reason(CoverageReason::UnsupportedResultShape);
                continue;
            };
            decisions += 1;
            would_shorten += u64::from(decision.would_chars_out < decision.chars_in);
        }

        let reasons = outcome
            .reasons
            .into_iter()
            .map(|(reason, count)| (reason.as_str().to_string(), count))
            .collect();
        let mut stats = self
            .stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        stats.requests_observed += 1;
        stats.exchanges_correlated += exchanges;
        stats.decisions_computed += decisions;
        stats.would_shorten += would_shorten;
        stats.last_coverage_reasons = reasons;
    }

    pub(super) fn health(&self) -> ShadowHealthReceipt {
        let stats = self
            .stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ShadowHealthReceipt {
            mode: "shadow",
            requests_observed: stats.requests_observed,
            exchanges_correlated: stats.exchanges_correlated,
            decisions_computed: stats.decisions_computed,
            would_shorten: stats.would_shorten,
            last_coverage_reasons: stats.last_coverage_reasons.clone(),
            raw_requests_persisted: false,
        }
    }
}

fn identity_encoded(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_ENCODING).iter();
    let Some(first) = values.next() else {
        return true;
    };
    values.next().is_none()
        && first
            .to_str()
            .is_ok_and(|value| value.eq_ignore_ascii_case("identity"))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn only_absent_or_identity_content_encoding_is_inspected() {
        let mut headers = HeaderMap::new();
        assert!(identity_encoded(&headers));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("identity"));
        assert!(identity_encoded(&headers));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(!identity_encoded(&headers));
    }
}
