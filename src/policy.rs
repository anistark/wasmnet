use ipnet::IpNet;
use serde::Deserialize;
use std::net::IpAddr;
use std::ops::RangeInclusive;
use std::str::FromStr;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub network: NetworkPolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkPolicy {
    #[serde(default = "default_deny")]
    pub deny: Vec<String>,
    #[serde(default = "default_allow")]
    pub allow: Vec<String>,
    #[serde(default = "default_bind_ports")]
    pub bind_ports: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_max_bandwidth_mbps")]
    pub max_bandwidth_mbps: u32,
    #[serde(default = "default_connection_timeout_secs")]
    pub connection_timeout_secs: u64,
}

fn default_deny() -> Vec<String> {
    vec![
        "10.0.0.0/8".into(),
        "172.16.0.0/12".into(),
        "192.168.0.0/16".into(),
        "127.0.0.0/8".into(),
        "169.254.0.0/16".into(),
    ]
}

fn default_allow() -> Vec<String> {
    vec!["*".into()]
}

fn default_bind_ports() -> String {
    "3000-9999".into()
}

fn default_max_connections() -> usize {
    50
}

fn default_max_bandwidth_mbps() -> u32 {
    10
}

fn default_connection_timeout_secs() -> u64 {
    30
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            deny: default_deny(),
            allow: default_allow(),
            bind_ports: default_bind_ports(),
            max_connections: default_max_connections(),
            max_bandwidth_mbps: default_max_bandwidth_mbps(),
            connection_timeout_secs: default_connection_timeout_secs(),
        }
    }
}

#[derive(Debug)]
pub struct Policy {
    deny_nets: Vec<NetRule>,
    deny_all: bool,
    allow_nets: Vec<NetRule>,
    allow_all: bool,
    allow_domains: Vec<DomainPattern>,
    deny_domains: Vec<DomainPattern>,
    bind_port_ranges: Vec<RangeInclusive<u16>>,
    pub max_connections: usize,
    pub connection_timeout_secs: u64,
}

/// A CIDR or bare-IP rule, optionally qualified by a port.
///
/// A rule with no port applies to every port: `10.0.0.0/8` covers the whole
/// range, `10.0.0.0/8:22` only covers SSH to it.
#[derive(Debug)]
struct NetRule {
    net: IpNet,
    port: Option<u16>,
}

impl NetRule {
    fn parse(s: &str) -> Option<Self> {
        // Try the whole string first, so an unbracketed IPv6 CIDR still parses.
        if let Some(net) = parse_net(s) {
            return Some(NetRule { net, port: None });
        }
        let (host, port) = split_host_port(s);
        port?;
        parse_net(host).map(|net| NetRule { net, port })
    }

    fn matches(&self, ip: &IpAddr, port: u16) -> bool {
        self.net.contains(ip) && self.port.is_none_or(|p| p == port)
    }
}

/// Parse a CIDR, or a bare IP address as a single-host network.
fn parse_net(s: &str) -> Option<IpNet> {
    if let Ok(net) = IpNet::from_str(s) {
        return Some(net);
    }
    IpAddr::from_str(s).ok().map(IpNet::from)
}

/// A domain rule, optionally qualified by a port.
///
/// A rule covers the domain itself and every subdomain of it, so `example.com`
/// and `*.example.com` mean the same thing. Names are compared case-folded and
/// with any single trailing dot removed, so the fully qualified `example.com.`
/// cannot slip past a rule written as `example.com`.
#[derive(Debug)]
struct DomainPattern {
    host: String,
    port: Option<u16>,
}

impl DomainPattern {
    fn parse(s: &str) -> Option<Self> {
        let (host, port) = split_host_port(s);
        let host = host.strip_prefix("*.").unwrap_or(host);
        let host = normalize_domain(host);
        if host.is_empty() || host == "*" {
            return None;
        }
        Some(DomainPattern { host, port })
    }

    fn matches(&self, domain: &str, port: u16) -> bool {
        if !self.port.is_none_or(|p| p == port) {
            return false;
        }
        let d = normalize_domain(domain);
        d == self.host || d.ends_with(&format!(".{}", self.host))
    }
}

/// Case-fold a name and drop the single trailing dot of the FQDN form.
fn normalize_domain(s: &str) -> String {
    s.strip_suffix('.').unwrap_or(s).to_lowercase()
}

fn split_host_port(s: &str) -> (&str, Option<u16>) {
    // Bracketed IPv6: `[::1]` or `[::1]:443`.
    if let Some(rest) = s.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        let port = rest[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok());
        return (&rest[..end], port);
    }
    if let Some(idx) = s.rfind(':') {
        // More than one colon means a bare IPv6 literal, not a port suffix.
        if s[..idx].contains(':') {
            return (s, None);
        }
        if let Ok(port) = s[idx + 1..].parse::<u16>() {
            return (&s[..idx], Some(port));
        }
    }
    (s, None)
}

fn parse_port_ranges(s: &str) -> Vec<RangeInclusive<u16>> {
    s.split(',')
        .filter_map(|part| {
            let part = part.trim();
            if let Some(idx) = part.find('-') {
                let lo = part[..idx].trim().parse().ok()?;
                let hi = part[idx + 1..].trim().parse().ok()?;
                Some(lo..=hi)
            } else {
                let p = part.parse().ok()?;
                Some(p..=p)
            }
        })
        .collect()
}

impl Policy {
    pub fn new(config: &NetworkPolicy) -> Self {
        let (deny_nets, deny_domains, deny_all) = parse_rules(&config.deny);
        let (allow_nets, allow_domains, allow_all) = parse_rules(&config.allow);

        Self {
            deny_nets,
            deny_all,
            allow_nets,
            allow_all,
            allow_domains,
            deny_domains,
            bind_port_ranges: parse_port_ranges(&config.bind_ports),
            max_connections: config.max_connections,
            connection_timeout_secs: config.connection_timeout_secs,
        }
    }

    /// Check a target as the caller spelled it.
    ///
    /// A literal IP is matched against the CIDR rules. A hostname is matched
    /// against the domain rules only: the addresses it resolves to are not
    /// known yet, so [`Policy::check_resolved`] has to be called for each of
    /// them before connecting.
    pub fn check_connect(&self, addr: &str, port: u16) -> Result<(), String> {
        match addr.parse::<IpAddr>() {
            Ok(ip) => self.check_ip(ip, port),
            Err(_) => self.check_domain(addr, port),
        }
    }

    /// Check an address a hostname resolved to.
    ///
    /// Only the CIDR rules apply here; the domain rules were already decided
    /// against the name in [`Policy::check_connect`]. A hostname that clears
    /// the domain rules still has to clear the deny list address by address,
    /// which is what stops `localhost` from reaching a blocked `127.0.0.1`.
    pub fn check_resolved(&self, ip: IpAddr, port: u16) -> Result<(), String> {
        for rule in &self.deny_nets {
            if rule.matches(&ip, port) {
                return Err(format!("private IP range blocked: {}", rule.net));
            }
        }
        Ok(())
    }

    fn check_ip(&self, ip: IpAddr, port: u16) -> Result<(), String> {
        for rule in &self.deny_nets {
            if rule.matches(&ip, port) {
                return Err(format!("private IP range blocked: {}", rule.net));
            }
        }
        if self.deny_all || !self.allow_all {
            let allowed = self.allow_nets.iter().any(|r| r.matches(&ip, port));
            if !allowed {
                return Err(self.reject(ip.to_string().as_str(), port));
            }
        }
        Ok(())
    }

    fn check_domain(&self, addr: &str, port: u16) -> Result<(), String> {
        for dp in &self.deny_domains {
            if dp.matches(addr, port) {
                return Err(format!("domain blocked by policy: {addr}"));
            }
        }
        if self.deny_all || !self.allow_all {
            let allowed = self.allow_domains.iter().any(|dp| dp.matches(addr, port));
            if !allowed {
                return Err(self.reject(addr, port));
            }
        }
        Ok(())
    }

    fn reject(&self, addr: &str, port: u16) -> String {
        if self.deny_all {
            format!("address blocked by policy (deny-all): {addr}:{port}")
        } else {
            format!("address not in allow list: {addr}:{port}")
        }
    }

    pub fn check_bind(&self, port: u16) -> Result<(), String> {
        for range in &self.bind_port_ranges {
            if range.contains(&port) {
                return Ok(());
            }
        }
        Err(format!("port {port} not in allowed bind range"))
    }

    pub fn allow_all() -> Self {
        Self {
            deny_nets: Vec::new(),
            deny_all: false,
            allow_nets: Vec::new(),
            allow_all: true,
            allow_domains: Vec::new(),
            deny_domains: Vec::new(),
            bind_port_ranges: vec![1..=65535],
            max_connections: usize::MAX,
            connection_timeout_secs: 30,
        }
    }
}

/// Sort one rule list into networks, domains, and the catch-all `*`.
fn parse_rules(entries: &[String]) -> (Vec<NetRule>, Vec<DomainPattern>, bool) {
    let mut nets = Vec::new();
    let mut domains = Vec::new();
    let mut all = false;

    for entry in entries {
        let entry = entry.trim();
        if entry == "*" {
            all = true;
        } else if let Some(rule) = NetRule::parse(entry) {
            nets.push(rule);
        } else if let Some(dp) = DomainPattern::parse(entry) {
            domains.push(dp);
        }
    }

    (nets, domains, all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(deny: &[&str], allow: &[&str]) -> Policy {
        Policy::new(&NetworkPolicy {
            deny: deny.iter().map(|s| s.to_string()).collect(),
            allow: allow.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        })
    }

    #[test]
    fn default_policy_blocks_private() {
        let policy = Policy::new(&NetworkPolicy::default());
        assert!(policy.check_connect("10.0.0.1", 22).is_err());
        assert!(policy.check_connect("192.168.1.1", 80).is_err());
        assert!(policy.check_connect("127.0.0.1", 8080).is_err());
    }

    #[test]
    fn default_policy_allows_public() {
        let policy = Policy::new(&NetworkPolicy::default());
        assert!(policy.check_connect("8.8.8.8", 53).is_ok());
        assert!(policy.check_connect("api.example.com", 443).is_ok());
    }

    #[test]
    fn bind_port_check() {
        let policy = Policy::new(&NetworkPolicy::default());
        assert!(policy.check_bind(3000).is_ok());
        assert!(policy.check_bind(9999).is_ok());
        assert!(policy.check_bind(80).is_err());
        assert!(policy.check_bind(22).is_err());
    }

    #[test]
    fn deny_all_with_allowlist() {
        let policy = policy(&["*"], &["api.example.com:443"]);
        assert!(policy.check_connect("api.example.com", 443).is_ok());
        assert!(policy.check_connect("evil.com", 80).is_err());
    }

    // ── resolved addresses are checked against the deny list ──

    #[test]
    fn resolved_address_hits_the_deny_list() {
        let policy = Policy::new(&NetworkPolicy::default());
        // The name itself clears the domain rules...
        assert!(policy.check_connect("localhost", 80).is_ok());
        // ...but what it resolves to does not.
        assert!(
            policy
                .check_resolved("127.0.0.1".parse().unwrap(), 80)
                .is_err()
        );
        assert!(
            policy
                .check_resolved("169.254.169.254".parse().unwrap(), 80)
                .is_err()
        );
        assert!(
            policy
                .check_resolved("8.8.8.8".parse().unwrap(), 80)
                .is_ok()
        );
    }

    // ── ports on rules ──

    #[test]
    fn domain_rule_port_is_enforced() {
        let policy = policy(&[], &["api.example.com:443"]);
        assert!(policy.check_connect("api.example.com", 443).is_ok());
        assert!(policy.check_connect("api.example.com", 22).is_err());
    }

    #[test]
    fn domain_rule_without_port_covers_every_port() {
        let policy = policy(&[], &["api.example.com"]);
        assert!(policy.check_connect("api.example.com", 443).is_ok());
        assert!(policy.check_connect("api.example.com", 22).is_ok());
    }

    #[test]
    fn deny_rule_port_is_enforced() {
        let policy = policy(&["evil.com:22"], &["*"]);
        assert!(policy.check_connect("evil.com", 22).is_err());
        assert!(policy.check_connect("evil.com", 443).is_ok());
    }

    #[test]
    fn cidr_rule_port_is_enforced() {
        let policy = policy(&["10.0.0.0/8:22"], &["*"]);
        assert!(policy.check_connect("10.1.2.3", 22).is_err());
        assert!(policy.check_connect("10.1.2.3", 443).is_ok());
    }

    #[test]
    fn bare_ip_rule_is_a_single_host_network() {
        let policy = policy(&["1.2.3.4"], &["*"]);
        assert!(policy.check_connect("1.2.3.4", 80).is_err());
        assert!(policy.check_connect("1.2.3.5", 80).is_ok());
    }

    #[test]
    fn bracketed_ipv6_rule_carries_a_port() {
        let policy = policy(&["[::1]:80", "fd00::/8"], &["*"]);
        assert!(policy.check_connect("::1", 80).is_err());
        assert!(policy.check_connect("::1", 443).is_ok());
        assert!(policy.check_connect("fd00::1", 443).is_err());
    }

    // ── domain matching ──

    #[test]
    fn deny_covers_subdomains_and_the_apex() {
        let policy = policy(&["evil.com", "*.blocked.com"], &["*"]);
        assert!(policy.check_connect("evil.com", 80).is_err());
        assert!(policy.check_connect("EVIL.com", 80).is_err());
        assert!(policy.check_connect("sub.evil.com", 80).is_err());
        assert!(policy.check_connect("deep.sub.evil.com", 80).is_err());
        assert!(policy.check_connect("blocked.com", 80).is_err());
        assert!(policy.check_connect("x.blocked.com", 80).is_err());
    }

    #[test]
    fn trailing_dot_does_not_escape_a_rule() {
        let policy = policy(&["evil.com"], &["*"]);
        assert!(policy.check_connect("evil.com.", 80).is_err());
        assert!(policy.check_connect("sub.evil.com.", 80).is_err());
    }

    #[test]
    fn allow_covers_subdomains_and_the_apex() {
        let policy = policy(&["*"], &["*.github.com"]);
        assert!(policy.check_connect("api.github.com", 443).is_ok());
        assert!(policy.check_connect("github.com", 443).is_ok());
        assert!(policy.check_connect("api.github.com.", 443).is_ok());
        assert!(policy.check_connect("notgithub.com", 443).is_err());
    }

    #[test]
    fn a_lookalike_suffix_does_not_match() {
        let policy = policy(&["evil.com"], &["*"]);
        assert!(policy.check_connect("notevil.com", 80).is_ok());
        assert!(policy.check_connect("evil.com.attacker.net", 80).is_ok());
    }

    #[test]
    fn deny_wins_over_allow() {
        let policy = policy(&["sub.example.com"], &["example.com"]);
        assert!(policy.check_connect("example.com", 443).is_ok());
        assert!(policy.check_connect("sub.example.com", 443).is_err());
    }
}
