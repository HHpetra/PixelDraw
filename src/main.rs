mod canvas;
mod palette;
mod server;

use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

use crate::server::PixelDraw;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    eprintln!("pixeldraw mcp ready");

    let service = PixelDraw::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
