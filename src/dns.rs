use tokio::net::lookup_host;

/// Resolve a hostname to a list of IP address strings.
pub async fn resolve(name: &str) -> Result<Vec<String>, String> {
    let with_port = if name.contains(':') {
        name.to_string()
    } else {
        format!("{name}:0")
    };

    let addrs = lookup_host(&with_port)
        .await
        .map_err(|e| format!("DNS resolution failed: {e}"))?;

    let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
    if ips.is_empty() {
        return Err(format!("no addresses found for {name}"));
    }
    Ok(ips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_localhost() {
        let addrs = resolve("localhost").await.unwrap();
        assert!(!addrs.is_empty());
        assert!(addrs.iter().any(|a| a == "127.0.0.1" || a == "::1"));
    }

    #[tokio::test]
    async fn resolve_nonexistent_fails() {
        let result = resolve("this.host.does.not.exist.invalid").await;
        assert!(result.is_err());
    }
}
