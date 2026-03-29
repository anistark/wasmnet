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

    #[arg(long, help = "Max bandwidth in Mbps (overrides policy file)")]
    max_bandwidth_mbps: Option<u32>,

    #[arg(
        long,
        help = "Enable connection pooling with this idle timeout (seconds)"
    )]
    pool_idle_secs: Option<u64>,

    #[arg(long, default_value_t = 8, help = "Max pooled connections per target")]
    pool_per_key: usize,
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

    if let Some(mbps) = args.max_bandwidth_mbps {
        info!("bandwidth limit: {mbps} Mbps");
        builder = builder.max_bandwidth_mbps(mbps);
    }

    if let Some(idle) = args.pool_idle_secs {
        info!(
            "connection pool: idle={idle}s, per_key={}",
            args.pool_per_key
        );
        builder = builder.pool(idle, args.pool_per_key);
    }

    let server = builder.build()?;
    server.listen().await?;
    Ok(())
}
