//! M6's content-free private-beta and commercial-readiness gate.

use anyhow::Result;
use serde::Serialize;

use super::lifecycle::{RegisteredRouteState, RouteStatus, ServiceState};
use crate::model_gateway::surfaces::FieldState;

const MIN_ACCEPTED_REQUESTS: i64 = 20;
const MAX_FAILURE_RATE: f64 = 0.001;
const MAX_LOCAL_P95_MS: i64 = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Gate {
    id: &'static str,
    passed: bool,
    required: bool,
    evidence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RouteReadiness {
    route: RouteStatus,
    evidence: crate::db::ModelGatewayRouteSummary,
    integrity: crate::db::ModelGatewayIntegrity,
    gates: Vec<Gate>,
    private_beta_ready: bool,
    status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalGate {
    id: &'static str,
    complete: bool,
    evidence: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadinessReport {
    schema: &'static str,
    generated_at: String,
    status: &'static str,
    route_private_beta_ready: bool,
    commercial_ready: bool,
    routes: Vec<RouteReadiness>,
    recovery: crate::db::ModelGatewayRecoverySummary,
    external_gates: Vec<ExternalGate>,
    unsupported_routes: Vec<&'static str>,
    raw_requests_persisted: bool,
    raw_request_scope: &'static str,
    credentials_persisted: bool,
}

pub async fn print(json: bool) -> Result<()> {
    let statuses = super::lifecycle::dashboard_status().await?;
    let (summaries, integrity, recovery) = match crate::db::open_db() {
        Ok(conn) => {
            crate::db::ensure_schema(&conn)?;
            (
                crate::db::model_gateway_route_summaries(&conn),
                crate::db::model_gateway_integrity(&conn),
                crate::db::model_gateway_recovery_summary(&conn),
            )
        }
        Err(_) => (Vec::new(), Vec::new(), Default::default()),
    };
    let routes = statuses
        .into_iter()
        .map(|status| {
            let evidence = summaries
                .iter()
                .find(|summary| evidence_matches(&status, summary))
                .cloned()
                .unwrap_or_else(|| empty_summary(&status));
            let integrity = integrity
                .iter()
                .find(|receipt| receipt.route_id == status.route_id)
                .cloned()
                .unwrap_or_else(|| crate::db::ModelGatewayIntegrity {
                    route_id: status.route_id.clone(),
                    ..Default::default()
                });
            evaluate_route(status, evidence, integrity)
        })
        .collect::<Vec<_>>();
    let route_private_beta_ready = !routes.is_empty()
        && routes
            .iter()
            .filter(|route| route.route.phase == "enabled")
            .all(|route| route.private_beta_ready)
        && routes.iter().any(|route| route.private_beta_ready);
    let external_gates = external_gates();
    let commercial_ready =
        route_private_beta_ready && external_gates.iter().all(|gate| gate.complete);
    let status = if routes.is_empty() {
        "no-routes"
    } else if commercial_ready {
        "commercial-ready"
    } else if route_private_beta_ready {
        "route-beta-ready-external-gates-open"
    } else {
        "collecting-route-proof"
    };
    let report = ReadinessReport {
        schema: "ctx.model-gateway-readiness.v1",
        generated_at: chrono::Utc::now().to_rfc3339(),
        status,
        route_private_beta_ready,
        commercial_ready,
        routes,
        recovery,
        external_gates,
        unsupported_routes: vec!["cursor-model-path", "provider-hosted-tools"],
        raw_requests_persisted: false,
        raw_request_scope: "model-gateway transport and receipt tables only; separate local CTX analytics and recovery stores have their own retention controls",
        credentials_persisted: false,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!("Model gateway readiness: {}", report.status);
    if report.routes.is_empty() {
        println!("  No model routes are registered or owned.");
    }
    for route in &report.routes {
        println!(
            "  {}: {} (attempted {}, accepted {}, applied {}, deadlines {}, local p95 {})",
            route.route.route_id,
            route.status,
            route.evidence.attempted,
            route.evidence.accepted,
            route.evidence.applied,
            route.evidence.transform_deadlines,
            route
                .evidence
                .p95_local_processing_ms
                .map(|value| format!("{value} ms"))
                .unwrap_or_else(|| "not measured".into())
        );
        for gate in route.gates.iter().filter(|gate| !gate.passed) {
            println!("    open: {} — {}", gate.id, gate.evidence);
        }
    }
    println!("  Commercial release: blocked until every external gate is complete.");
    for gate in report.external_gates.iter().filter(|gate| !gate.complete) {
        println!("    open: {} — {}", gate.id, gate.evidence);
    }
    Ok(())
}

fn evaluate_route(
    status: RouteStatus,
    evidence: crate::db::ModelGatewayRouteSummary,
    integrity: crate::db::ModelGatewayIntegrity,
) -> RouteReadiness {
    let failure_count =
        evidence.provider_rejected + evidence.transport_failures + evidence.processing_failures;
    let failure_rate = if evidence.attempted > 0 {
        failure_count as f64 / evidence.attempted as f64
    } else {
        1.0
    };
    let mut gates = vec![
        Gate {
            id: "exact-lifecycle-health",
            passed: status.phase == "enabled"
                && status.config_state == FieldState::CtxOwned
                && status.service_state == ServiceState::Healthy
                && status.registered_route_state == RegisteredRouteState::Matching,
            required: true,
            evidence: format!(
                "phase={}, config={:?}, service={:?}, registry={:?}",
                status.phase,
                status.config_state,
                status.service_state,
                status.registered_route_state
            ),
        },
        Gate {
            id: "captured-client-version",
            passed: status.client_version.is_some(),
            required: true,
            evidence: status
                .client_version
                .clone()
                .unwrap_or_else(|| "no enabled client-version receipt".into()),
        },
        Gate {
            id: "provider-acceptance-corpus",
            passed: evidence.accepted >= MIN_ACCEPTED_REQUESTS,
            required: true,
            evidence: format!(
                "{} of {} accepted requests required",
                evidence.accepted, MIN_ACCEPTED_REQUESTS
            ),
        },
        Gate {
            id: "transport-reliability",
            passed: evidence.attempted >= MIN_ACCEPTED_REQUESTS && failure_rate <= MAX_FAILURE_RATE,
            required: true,
            evidence: format!(
                "{} failures across {} attempts ({:.2}%; limit {:.2}%)",
                failure_count,
                evidence.attempted,
                failure_rate * 100.0,
                MAX_FAILURE_RATE * 100.0
            ),
        },
        Gate {
            id: "local-processing-p95",
            passed: evidence
                .p95_local_processing_ms
                .is_some_and(|latency| latency <= MAX_LOCAL_P95_MS),
            required: true,
            evidence: evidence
                .p95_local_processing_ms
                .map(|latency| format!("{latency} ms p95; limit {MAX_LOCAL_P95_MS} ms"))
                .unwrap_or_else(|| "not measured".into()),
        },
        Gate {
            id: "exact-recovery",
            passed: integrity.applied_without_recovery == 0
                && integrity.applied_decisions == evidence.applied,
            required: true,
            evidence: format!(
                "{} applied decisions, {} applied receipts, {} missing originals",
                integrity.applied_decisions, evidence.applied, integrity.applied_without_recovery
            ),
        },
    ];
    if status.mode == "testing" {
        gates.push(Gate {
            id: "upstream-accepted-trim",
            passed: evidence.applied > 0 && evidence.chars_saved > 0,
            required: true,
            evidence: format!(
                "{} applied trims, {} exact characters removed",
                evidence.applied, evidence.chars_saved
            ),
        });
    }
    let private_beta_ready = gates
        .iter()
        .filter(|gate| gate.required)
        .all(|gate| gate.passed);
    let status_label = if status.phase != "enabled" {
        "inactive"
    } else if private_beta_ready && status.mode == "testing" {
        "testing-beta-ready"
    } else if private_beta_ready {
        "shadow-beta-ready"
    } else {
        "collecting"
    };
    RouteReadiness {
        route: status,
        evidence,
        integrity,
        gates,
        private_beta_ready,
        status: status_label,
    }
}

fn empty_summary(status: &RouteStatus) -> crate::db::ModelGatewayRouteSummary {
    crate::db::ModelGatewayRouteSummary {
        route_id: status.route_id.clone(),
        surface: status.surface.clone(),
        surface_version: status.client_version.clone(),
        protocol: status.protocol.clone(),
        authentication: status.authentication.clone(),
        fixed_upstream: status.fixed_upstream.clone(),
        mode: status.mode.clone(),
        ..Default::default()
    }
}

fn evidence_matches(status: &RouteStatus, summary: &crate::db::ModelGatewayRouteSummary) -> bool {
    summary.route_id == status.route_id
        && summary.surface == status.surface
        && summary.surface_version == status.client_version
        && summary.protocol == status.protocol
        && summary.authentication == status.authentication
        && summary.fixed_upstream == status.fixed_upstream
        && summary.mode == status.mode
}

fn external_gates() -> Vec<ExternalGate> {
    vec![
        ExternalGate {
            id: "macos-live-corpus",
            complete: false,
            evidence: "clean and customized beta-user lifecycle corpus not attached",
        },
        ExternalGate {
            id: "linux-live-corpus",
            complete: false,
            evidence: "systemd and provider lifecycle corpus not attached",
        },
        ExternalGate {
            id: "cache-adjusted-value",
            complete: false,
            evidence: "real provider cached-input comparison not complete",
        },
        ExternalGate {
            id: "signed-artifacts",
            complete: false,
            evidence: "current beta artifacts remain unsigned/unnotarized",
        },
        ExternalGate {
            id: "sbom-and-dependency-audit",
            complete: false,
            evidence: "release-bound SBOM and audit receipt not attached",
        },
        ExternalGate {
            id: "independent-security-review",
            complete: false,
            evidence: "independent model-gateway review not complete",
        },
        ExternalGate {
            id: "beta-cohort-and-comprehension",
            complete: false,
            evidence: "cohort value and first-time-user comprehension gates not run",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_status(mode: &str) -> RouteStatus {
        RouteStatus {
            route_id: "codex-api".into(),
            surface: "codex".into(),
            phase: "enabled".into(),
            mode: mode.into(),
            authentication: "api-key".into(),
            protocol: "openai-responses".into(),
            fixed_upstream: "https://api.openai.com".into(),
            local_base_url: "http://127.0.0.1:8871/v1".into(),
            config_location: None,
            config_state: FieldState::CtxOwned,
            service_state: ServiceState::Healthy,
            registered_route_state: RegisteredRouteState::Matching,
            client_version: Some("codex-cli 1.2.3".into()),
            credentials_persisted: false,
            cursor_model_path_available: false,
            process_visibility: "in-memory request",
            retained_locally: "content-free receipts and prepared originals",
            cloud_relay: false,
            controlled_path: "local tool results",
            unavailable_path: "hosted tools",
            cache_accounting: "pending",
            recovery_command: "ctx expand <rewind-id>",
            purge_control: "Settings",
            bypass_command: Some("ctx model-gateway bypass codex-api".into()),
        }
    }

    #[test]
    fn testing_route_needs_real_acceptance_recovery_reliability_and_latency() {
        let status = healthy_status("testing");
        let evidence = crate::db::ModelGatewayRouteSummary {
            route_id: "codex-api".into(),
            attempted: 20,
            accepted: 20,
            applied: 3,
            chars_saved: 25_000,
            p95_local_processing_ms: Some(15),
            ..empty_summary(&status)
        };
        let integrity = crate::db::ModelGatewayIntegrity {
            route_id: "codex-api".into(),
            applied_decisions: 3,
            applied_without_recovery: 0,
        };
        let ready = evaluate_route(status, evidence, integrity);
        assert!(ready.private_beta_ready);
        assert_eq!(ready.status, "testing-beta-ready");
    }

    #[test]
    fn receipts_without_exact_recovery_fail_closed() {
        let status = healthy_status("testing");
        let evidence = crate::db::ModelGatewayRouteSummary {
            route_id: "codex-api".into(),
            attempted: 20,
            accepted: 20,
            applied: 1,
            chars_saved: 1_000,
            p95_local_processing_ms: Some(10),
            ..empty_summary(&status)
        };
        let readiness = evaluate_route(
            status,
            evidence,
            crate::db::ModelGatewayIntegrity {
                route_id: "codex-api".into(),
                applied_decisions: 1,
                applied_without_recovery: 1,
            },
        );
        assert!(!readiness.private_beta_ready);
        assert!(readiness
            .gates
            .iter()
            .any(|gate| gate.id == "exact-recovery" && !gate.passed));
    }

    #[test]
    fn historical_evidence_from_another_client_version_never_activates_a_route() {
        let status = healthy_status("testing");
        let mut historical = empty_summary(&status);
        historical.surface_version = Some("codex-cli 1.2.2".into());
        assert!(!evidence_matches(&status, &historical));
        historical.surface_version = status.client_version.clone();
        assert!(evidence_matches(&status, &historical));
    }
}
