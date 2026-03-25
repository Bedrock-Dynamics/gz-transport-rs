//! Configuration for gz-transport nodes.

use std::net::Ipv4Addr;
use std::time::Duration;

/// Default multicast address for discovery.
pub const DEFAULT_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 0, 7);

/// Default message discovery port.
pub const DEFAULT_MSG_DISCOVERY_PORT: u16 = 10317;

/// Default service discovery port.
pub const DEFAULT_SRV_DISCOVERY_PORT: u16 = 10318;

/// Protocol wire version.
pub const WIRE_VERSION: u32 = 10;

/// Heartbeat interval in milliseconds.
pub const HEARTBEAT_INTERVAL_MS: u64 = 1000;

/// Silence timeout in milliseconds (node considered dead).
pub const SILENCE_TIMEOUT_MS: u64 = 3000;

/// Activity check interval in milliseconds.
pub const ACTIVITY_INTERVAL_MS: u64 = 100;

/// Configuration for a gz-transport node.
#[derive(Debug, Clone)]
pub struct Config {
    /// Partition name (default: from GZ_PARTITION or hostname:username).
    pub partition: Option<String>,
    /// Discovery multicast address.
    pub multicast_addr: Ipv4Addr,
    /// Message discovery port.
    pub msg_discovery_port: u16,
    /// Service discovery port.
    pub srv_discovery_port: u16,
    /// Discovery timeout.
    pub discovery_timeout: Duration,
    /// Service call timeout.
    pub service_timeout: Duration,
    /// Heartbeat interval.
    pub heartbeat_interval: Duration,
    /// Host IP override (default: auto-detect).
    pub host_ip: Option<Ipv4Addr>,
    /// Service response port (default: ephemeral).
    /// Set this to a specific port to avoid conflicts with VPNs like Tailscale.
    pub service_response_port: Option<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            partition: None,
            multicast_addr: DEFAULT_MULTICAST_ADDR,
            msg_discovery_port: DEFAULT_MSG_DISCOVERY_PORT,
            srv_discovery_port: DEFAULT_SRV_DISCOVERY_PORT,
            discovery_timeout: Duration::from_secs(5),
            service_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_millis(HEARTBEAT_INTERVAL_MS),
            host_ip: None,
            service_response_port: None,
        }
    }
}

impl Config {
    /// Create a new configuration builder.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// Load configuration from environment variables.
    ///
    /// Reads: `GZ_PARTITION`, `GZ_DISCOVERY_MULTICAST_IP`,
    /// `GZ_DISCOVERY_MSG_PORT`, `GZ_DISCOVERY_SRV_PORT`, `GZ_IP`,
    /// `GZ_SERVICE_RESPONSE_PORT`
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(partition) = std::env::var("GZ_PARTITION") {
            config.partition = Some(partition);
        }

        if let Ok(addr) = std::env::var("GZ_DISCOVERY_MULTICAST_IP") {
            if let Ok(ip) = addr.parse() {
                config.multicast_addr = ip;
            }
        }

        if let Ok(port) = std::env::var("GZ_DISCOVERY_MSG_PORT") {
            if let Ok(p) = port.parse() {
                config.msg_discovery_port = p;
            }
        }

        if let Ok(port) = std::env::var("GZ_DISCOVERY_SRV_PORT") {
            if let Ok(p) = port.parse() {
                config.srv_discovery_port = p;
            }
        }

        if let Ok(ip) = std::env::var("GZ_IP") {
            if let Ok(addr) = ip.parse() {
                config.host_ip = Some(addr);
            }
        }

        if let Ok(port) = std::env::var("GZ_SERVICE_RESPONSE_PORT") {
            if let Ok(p) = port.parse() {
                config.service_response_port = Some(p);
            }
        }

        config
    }

    /// Get the effective partition name.
    ///
    /// Returns the configured partition, or generates a default
    /// from hostname and username.
    pub fn effective_partition(&self) -> String {
        if let Some(ref p) = self.partition {
            return p.clone();
        }

        // Default: hostname:username
        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "localhost".to_string());

        let username = whoami::username();

        format!("{}:{}", hostname, username)
    }
}

/// Builder for [`Config`].
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Set the partition name.
    pub fn partition(mut self, partition: Option<&str>) -> Self {
        self.config.partition = partition.map(String::from);
        self
    }

    /// Set the multicast address.
    pub fn multicast_addr(mut self, addr: Ipv4Addr) -> Self {
        self.config.multicast_addr = addr;
        self
    }

    /// Set the message discovery port.
    pub fn msg_discovery_port(mut self, port: u16) -> Self {
        self.config.msg_discovery_port = port;
        self
    }

    /// Set the service discovery port.
    pub fn srv_discovery_port(mut self, port: u16) -> Self {
        self.config.srv_discovery_port = port;
        self
    }

    /// Set discovery timeout.
    pub fn discovery_timeout(mut self, timeout: Duration) -> Self {
        self.config.discovery_timeout = timeout;
        self
    }

    /// Set service call timeout.
    pub fn service_timeout(mut self, timeout: Duration) -> Self {
        self.config.service_timeout = timeout;
        self
    }

    /// Set heartbeat interval.
    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.config.heartbeat_interval = interval;
        self
    }

    /// Set host IP override.
    pub fn host_ip(mut self, ip: Option<Ipv4Addr>) -> Self {
        self.config.host_ip = ip;
        self
    }

    /// Set service response port.
    /// Use this to avoid conflicts with VPNs like Tailscale.
    pub fn service_response_port(mut self, port: Option<u16>) -> Self {
        self.config.service_response_port = port;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Config {
        self.config
    }
}
