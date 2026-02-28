use std::net::SocketAddr;

use clap::Parser;
use tracing::info;

use wasmnet::policy::PolicyConfig;

#[derive(Parser)]
#[command(name = "wasmnet-server", about = "Networking proxy for browser WASM")]
struct Args {
    #[arg(short, long, default_value = "0.0.0.0")]
    host: String,

    #[arg(short, long, default_value_t = 9000)]
    port: u16,

    #[arg(long, help = "Path to policy TOML file")]
    policy: Option<String>,

    #[arg(long, help = "Disable all policy checks (allow everything)")]
    no_policy: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wasmnet=info".into()),
        )
        .init();

    let args = Args::parse();
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    let server = if args.no_policy {
        info!("starting with all policy checks disabled");
        wasmnet::Server::allow_all(addr)
    } else if let Some(path) = &args.policy {
        let content = std::fs::read_to_string(path)?;
        let config: PolicyConfig = toml::from_str(&content)?;
        info!("loaded policy from {path}");
        wasmnet::Server::from_config(config, addr)
    } else {
        info!("using default policy");
        wasmnet::Server::from_config(PolicyConfig::default(), addr)
    };

    server.listen().await?;
    Ok(())
}
