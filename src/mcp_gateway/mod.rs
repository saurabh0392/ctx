//! Local MCP gateway. Tool traffic stays between the agent, this process, and the user-approved
//! destination; CTX does not operate a relay service.

mod http;
pub mod oauth;
pub mod registry;
mod stdio;

use anyhow::Result;

pub async fn serve(server_id: &str, surface: &str) -> Result<()> {
    let registry = registry::GatewayRegistry::load()?;
    let server = registry
        .servers
        .get(server_id)
        .ok_or_else(|| anyhow::anyhow!("gateway server {server_id:?} is not registered"))?;
    match server {
        registry::GatewayServer::Stdio(server) => stdio::serve(server_id, surface, server).await,
        registry::GatewayServer::StreamableHttp(server) => {
            http::serve(server_id, surface, server).await
        }
    }
}
