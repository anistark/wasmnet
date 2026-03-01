use clap::Parser;
use tracing::info;

#[derive(Parser)]
#[command(name = "wasmnet-server", about = "Networking proxy for browser WASM")]
struct Args {
    #[arg(short = 'H', long, default_value = "0.0.0.0")]
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

    let mut builder = wasmnet::Server::builder().host(&args.host).port(args.port);

    if args.no_policy {
        info!("starting with all policy checks disabled");
        builder = builder.no_policy();
    } else if let Some(path) = &args.policy {
        info!("loading policy from {path}");
        builder = builder.policy_file(path)?;
    } else {
        info!("using default policy");
    }

    let server = builder.build()?;
    server.listen().await?;
    Ok(())
}
