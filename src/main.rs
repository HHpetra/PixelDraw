mod canvas;
mod palette;
mod pattern;
mod server;
mod shapes;

use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

use crate::server::PixelDraw;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mut output_dir = std::env::current_dir()?.join("output");
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--output-dir") => {
                let value = args.next().ok_or("--output-dir requires a path")?;
                if value.is_empty() {
                    return Err("--output-dir requires a nonempty path".into());
                }
                output_dir = std::path::PathBuf::from(value);
            }
            Some("--help" | "-h") => {
                println!(
                    "PixelDraw: stdio MCP pixel art server\n\nUsage: pixeldraw [--output-dir PATH]\n\nDefault output: ./output relative to the working directory.\nExisting files are never overwritten."
                );
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("pixeldraw {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => return Err(format!("Unknown argument: {}", arg.to_string_lossy()).into()),
        }
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    eprintln!("pixeldraw mcp ready");

    let service = PixelDraw::new(output_dir).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
