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
    deny_nets: Vec<IpNet>,
    deny_all: bool,
    allow_nets: Vec<IpNet>,
    allow_all: bool,
    allow_domains: Vec<DomainPattern>,
    deny_domains: Vec<DomainPattern>,
    bind_port_ranges: Vec<RangeInclusive<u16>>,
    pub max_connections: usize,
    pub connection_timeout_secs: u64,
}

#[derive(Debug)]
enum DomainPattern {
    Exact(String),
    Wildcard(String),
}

impl DomainPattern {
    fn parse(s: &str) -> Option<Self> {
        let (host, _port) = split_host_port(s);
        if host == "*" {
            return None;
        }
        if host.starts_with("*.") {
            Some(DomainPattern::Wildcard(host[1..].to_lowercase()))
        } else {
            Some(DomainPattern::Exact(host.to_lowercase()))
        }
    }

    fn matches(&self, domain: &str) -> bool {
        let d = domain.to_lowercase();
        match self {
            DomainPattern::Exact(e) => d == *e,
            DomainPattern::Wildcard(suffix) => d.ends_with(suffix.as_str()),
        }
    }
}

fn split_host_port(s: &str) -> (&str, Option<u16>) {
    if let Some(idx) = s.rfind(':') {
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
        let mut deny_nets = Vec::new();
        let mut deny_all = false;
        let mut deny_domains = Vec::new();

        for entry in &config.deny {
            if entry == "*" {
                deny_all = true;
            } else if let Ok(net) = IpNet::from_str(entry) {
                deny_nets.push(net);
            } else if let Some(dp) = DomainPattern::parse(entry) {
                deny_domains.push(dp);
            }
        }

        let mut allow_nets = Vec::new();
        let mut allow_all = false;
        let mut allow_domains = Vec::new();

        for entry in &config.allow {
            if entry == "*" {
                allow_all = true;
            } else if let Ok(net) = IpNet::from_str(entry) {
                allow_nets.push(net);
            } else if let Some(dp) = DomainPattern::parse(entry) {
                allow_domains.push(dp);
            }
        }

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

    pub fn check_connect(&self, addr: &str, port: u16) -> Result<(), String> {
        if self.deny_all {
            if !self.is_allowed_domain(addr, port) {
                return Err("address blocked by policy (deny-all)".into());
            }
            return Ok(());
        }

        if let Ok(ip) = addr.parse::<IpAddr>() {
            for net in &self.deny_nets {
                if net.contains(&ip) {
                    return Err(format!("private IP range blocked: {net}"));
                }
            }
        } else {
            for dp in &self.deny_domains {
                if dp.matches(addr) {
                    return Err(format!("domain blocked by policy: {addr}"));
                }
            }
        }

        if self.allow_all {
            return Ok(());
        }

        if let Ok(ip) = addr.parse::<IpAddr>() {
            for net in &self.allow_nets {
                if net.contains(&ip) {
                    return Ok(());
                }
            }
        }

        if self.is_allowed_domain(addr, port) {
            return Ok(());
        }

        Err(format!("address not in allow list: {addr}:{port}"))
    }

    fn is_allowed_domain(&self, addr: &str, port: u16) -> bool {
        for dp in &self.allow_domains {
            if dp.matches(addr) {
                return true;
            }
        }
        let with_port = format!("{addr}:{port}");
        for entry in &self.allow_nets {
            if entry.to_string() == with_port {
                return true;
            }
        }
        false
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let config = NetworkPolicy {
            deny: vec!["*".into()],
            allow: vec!["api.example.com:443".into()],
            ..Default::default()
        };
        let policy = Policy::new(&config);
        assert!(policy.check_connect("api.example.com", 443).is_ok());
        assert!(policy.check_connect("evil.com", 80).is_err());
    }
}
