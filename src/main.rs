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

    #[cfg(feature = "webtransport")]
    #[arg(long, help = "Also serve WebTransport (HTTP/3) on this UDP port")]
    webtransport_port: Option<u16>,

    #[cfg(feature = "webtransport")]
    #[arg(long, help = "TLS certificate PEM file for WebTransport (else self-signed)")]
    cert: Option<String>,

    #[cfg(feature = "webtransport")]
    #[arg(long, help = "TLS private key PEM file for WebTransport")]
    key: Option<String>,
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

    #[cfg(feature = "webtransport")]
    if let Some(wt_port) = args.webtransport_port {
        use std::path::Path;
        let wt_addr = format!("{}:{}", args.host, wt_port).parse()?;
        let cert = args.cert.as_deref().map(Path::new);
        let key = args.key.as_deref().map(Path::new);
        info!("also serving WebTransport on {wt_addr}");
        tokio::select! {
            r = server.listen() => r?,
            r = server.listen_webtransport(wt_addr, cert, key) => r?,
        }
        return Ok(());
    }

    server.listen().await?;
    Ok(())
}
