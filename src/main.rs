use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    mmr::app::run().await
}
