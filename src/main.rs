use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ctx::run().await
}
