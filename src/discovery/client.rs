//! Discovery client for UDP multicast.

use crate::config::{Config, SILENCE_TIMEOUT_MS, WIRE_VERSION};
use crate::discovery::message::{decode_frame, encode_frame};
use crate::discovery::registry::{Registry, ServiceInfo, SharedRegistry, TopicInfo};
use crate::error::{Error, Result};
use crate::msgs::{Discovery, discovery};
use crate::partition::fully_qualified_name;

use prost::Message;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

/// Message types for the discovery sender task
enum DiscoveryRequest {
    Subscribe(String),
    NewConnection {
        topic: String,
        /// Publisher's ctrl address (their pUUID) - so they know the message is for them
        publisher_ctrl: String,
    },
    Advertise {
        topic: String,
        msg_type: String,
        address: String,
    },
}

/// A publisher registered by this process, re-advertised periodically.
#[derive(Clone)]
struct OurPublisher {
    topic: String,
    msg_type: String,
    address: String,
}

/// Discovery client for finding Gazebo topics and services.
pub struct DiscoveryClient {
    config: Config,
    registry: SharedRegistry,
    process_uuid: String,
    running: Arc<AtomicBool>,
    /// Channel for sending discovery requests (SUBSCRIBE, NEW_CONNECTION, ADVERTISE)
    request_tx: Option<mpsc::Sender<DiscoveryRequest>>,
    /// Publishers registered by this process, re-advertised periodically.
    our_publishers: Arc<parking_lot::RwLock<Vec<OurPublisher>>>,
}

impl DiscoveryClient {
    /// Create a new discovery client.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            registry: Arc::new(Registry::new()),
            process_uuid: uuid::Uuid::new_v4().to_string(),
            running: Arc::new(AtomicBool::new(false)),
            request_tx: None,
            our_publishers: Arc::new(parking_lot::RwLock::new(Vec::new())),
        }
    }

    /// Start the discovery client.
    ///
    /// Spawns background tasks for:
    /// - Receiving topic discovery messages (port 10317)
    /// - Receiving service discovery messages (port 10318)
    /// - Sending heartbeats
    /// - Cleaning up stale entries
    pub async fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let partition = self.config.effective_partition();

        debug!(
            "[GZ-DISCOVERY] Starting discovery client on partition '{}' (multicast {}:{} topics, {} services)",
            partition,
            self.config.multicast_addr,
            self.config.msg_discovery_port,
            self.config.srv_discovery_port
        );

        // Create UDP socket for TOPIC discovery (port 10317)
        let msg_socket =
            create_multicast_socket(self.config.multicast_addr, self.config.msg_discovery_port)?;
        let msg_socket = Arc::new(tokio::net::UdpSocket::from_std(msg_socket)?);

        // Create UDP socket for SERVICE discovery (port 10318)
        // CRITICAL: Services like EntityFactory are advertised on this separate port!
        let srv_socket =
            create_multicast_socket(self.config.multicast_addr, self.config.srv_discovery_port)?;
        let srv_socket = Arc::new(tokio::net::UdpSocket::from_std(srv_socket)?);

        self.running.store(true, Ordering::Relaxed);

        // Channel for discovery requests (SUBSCRIBE and NEW_CONNECTION)
        let (request_tx, request_rx) = mpsc::channel::<DiscoveryRequest>(100);
        self.request_tx = Some(request_tx);

        // Spawn TOPIC receiver task (port 10317)
        let recv_socket = msg_socket.clone();
        let recv_registry = self.registry.clone();
        let recv_running = self.running.clone();
        let recv_partition = self.config.effective_partition();

        tokio::spawn(async move {
            receiver_task(
                recv_socket,
                recv_registry,
                recv_running,
                recv_partition,
                "topics",
            )
            .await;
        });

        // Spawn SERVICE receiver task (port 10318)
        let srv_recv_registry = self.registry.clone();
        let srv_recv_running = self.running.clone();
        let srv_recv_partition = self.config.effective_partition();

        tokio::spawn(async move {
            receiver_task(
                srv_socket,
                srv_recv_registry,
                srv_recv_running,
                srv_recv_partition,
                "services",
            )
            .await;
        });

        // Spawn heartbeat task (uses topic discovery port)
        let hb_socket = msg_socket.clone();
        let hb_running = self.running.clone();
        let hb_interval = self.config.heartbeat_interval;
        let hb_process_uuid = self.process_uuid.clone();
        let hb_multicast =
            SocketAddrV4::new(self.config.multicast_addr, self.config.msg_discovery_port);

        tokio::spawn(async move {
            heartbeat_task(
                hb_socket,
                hb_running,
                hb_interval,
                hb_process_uuid,
                hb_multicast,
            )
            .await;
        });

        // Spawn cleanup task
        let cleanup_registry = self.registry.clone();
        let cleanup_running = self.running.clone();

        tokio::spawn(async move {
            cleanup_task(cleanup_registry, cleanup_running).await;
        });

        // Spawn discovery sender task (handles SUBSCRIBE and NEW_CONNECTION)
        let sender_socket = msg_socket;
        let sender_running = self.running.clone();
        let sender_process_uuid = self.process_uuid.clone();
        let sender_partition = self.config.effective_partition();
        let sender_multicast =
            SocketAddrV4::new(self.config.multicast_addr, self.config.msg_discovery_port);

        tokio::spawn(async move {
            discovery_sender_task(
                sender_socket,
                request_rx,
                sender_running,
                sender_process_uuid,
                sender_partition,
                sender_multicast,
            )
            .await;
        });

        // Spawn periodic re-advertisement task for our publishers
        let adv_publishers = Arc::clone(&self.our_publishers);
        let adv_socket =
            create_multicast_socket(self.config.multicast_addr, self.config.msg_discovery_port)?;
        let adv_socket = tokio::net::UdpSocket::from_std(adv_socket)?;
        let adv_running = Arc::clone(&self.running);
        let adv_partition = self.config.effective_partition();
        let adv_process_uuid = self.process_uuid.clone();
        let adv_multicast_target = std::net::SocketAddr::new(
            std::net::IpAddr::V4(self.config.multicast_addr),
            self.config.msg_discovery_port,
        );

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            while adv_running.load(Ordering::Relaxed) {
                interval.tick().await;
                let publishers = adv_publishers.read().clone();
                for pub_info in &publishers {
                    let fq = fully_qualified_name(&adv_partition, "", &pub_info.topic);
                    let discovery_msg = Discovery {
                        header: None,
                        version: WIRE_VERSION,
                        r#type: discovery::Type::Advertise as i32,
                        disc_contents: Some(discovery::DiscContents::Pub(discovery::Publisher {
                            topic: fq,
                            address: pub_info.address.clone(),
                            process_uuid: adv_process_uuid.clone(),
                            node_uuid: adv_process_uuid.clone(),
                            scope: discovery::publisher::Scope::All as i32,
                            pub_type: Some(discovery::publisher::PubType::MsgPub(
                                discovery::MessagePublisher {
                                    ctrl: adv_process_uuid.clone(),
                                    msg_type: pub_info.msg_type.clone(),
                                    throttled: false,
                                    msgs_per_sec: 0,
                                },
                            )),
                        })),
                        process_uuid: adv_process_uuid.clone(),
                        flags: None,
                    };
                    if let Ok(frame) = encode_frame(&discovery_msg) {
                        let _ = adv_socket.send_to(&frame, adv_multicast_target).await;
                    }
                }
            }
        });

        // Wait for initial discovery
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    /// Stop the discovery client.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        self.request_tx = None;
    }

    /// Send a subscribe request for a topic.
    pub async fn subscribe(&self, topic: &str) -> Result<()> {
        if let Some(ref tx) = self.request_tx {
            tx.send(DiscoveryRequest::Subscribe(topic.to_string()))
                .await
                .map_err(|_| Error::ChannelClosed)?;
        }
        Ok(())
    }

    /// Send a NEW_CONNECTION message to notify the publisher that we're connected.
    ///
    /// This is required for gz-transport publishers to actually send data to remote subscribers.
    /// Without this message, publishers skip serialization because remoteSubscribers is empty.
    ///
    /// # Arguments
    /// * `topic` - The topic we're subscribing to
    /// * `publisher_process_uuid` - The publisher's pUUID (used as ctrl to address the message)
    pub async fn notify_connection(&self, topic: &str, publisher_process_uuid: &str) -> Result<()> {
        debug!(
            "notify_connection for topic '{}', publisher_ctrl: '{}'",
            topic, publisher_process_uuid
        );

        if let Some(ref tx) = self.request_tx {
            tx.send(DiscoveryRequest::NewConnection {
                topic: topic.to_string(),
                publisher_ctrl: publisher_process_uuid.to_string(),
            })
            .await
            .map_err(|_| Error::ChannelClosed)?;
            debug!("NEW_CONNECTION queued for '{}'", topic);
        } else {
            warn!("request_tx is None, cannot send NEW_CONNECTION");
        }
        Ok(())
    }

    /// Register a publisher with the discovery network.
    ///
    /// Sends an ADVERTISE message to the multicast group so subscribers
    /// can discover and connect to this publisher.
    pub async fn advertise(&self, topic: &str, msg_type: &str, address: &str) -> Result<()> {
        // Store for periodic re-advertisement.
        self.our_publishers.write().push(OurPublisher {
            topic: topic.to_string(),
            msg_type: msg_type.to_string(),
            address: address.to_string(),
        });

        // Send immediate ADVERTISE via the sender task.
        if let Some(ref tx) = self.request_tx {
            tx.send(DiscoveryRequest::Advertise {
                topic: topic.to_string(),
                msg_type: msg_type.to_string(),
                address: address.to_string(),
            })
            .await
            .map_err(|_| Error::ChannelClosed)?;
        }
        Ok(())
    }

    /// Get our process UUID (needed for some protocol operations).
    #[allow(dead_code)] // Part of public API
    pub fn process_uuid(&self) -> &str {
        &self.process_uuid
    }

    /// Get the shared registry.
    #[allow(dead_code)] // Part of public API
    pub fn registry(&self) -> SharedRegistry {
        self.registry.clone()
    }

    /// Get topic info, waiting for discovery if needed.
    pub async fn get_topic(&self, topic: &str, timeout: Duration) -> Result<TopicInfo> {
        // First, send a subscribe request
        self.subscribe(topic).await?;

        // Wait for discovery
        let start = std::time::Instant::now();
        loop {
            if let Some(info) = self.registry.get_topic(topic) {
                if !info.publishers.is_empty() {
                    return Ok(info);
                }
            }

            if start.elapsed() >= timeout {
                return Err(Error::DiscoveryTimeout {
                    topic: topic.to_string(),
                });
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Get service info, waiting for discovery if needed.
    pub async fn get_service(&self, service: &str, timeout: Duration) -> Result<ServiceInfo> {
        // Wait for discovery
        let start = std::time::Instant::now();
        loop {
            if let Some(info) = self.registry.get_service(service) {
                return Ok(info);
            }

            if start.elapsed() >= timeout {
                return Err(Error::ServiceNotFound(service.to_string()));
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// List all discovered topics.
    pub fn topics(&self) -> Vec<TopicInfo> {
        self.registry.list_topics()
    }

    /// List all discovered services.
    pub fn services(&self) -> Vec<ServiceInfo> {
        self.registry.list_services()
    }
}

impl Drop for DiscoveryClient {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Create a UDP socket configured for multicast.
fn create_multicast_socket(multicast_addr: Ipv4Addr, port: u16) -> Result<std::net::UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

    // Allow address reuse
    socket.set_reuse_address(true)?;

    #[cfg(unix)]
    socket.set_reuse_port(true)?;

    // Bind to any address on the discovery port
    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    socket.bind(&bind_addr.into())?;

    // Join multicast group
    socket.join_multicast_v4(&multicast_addr, &Ipv4Addr::UNSPECIFIED)?;

    // Set multicast TTL
    socket.set_multicast_ttl_v4(1)?;

    // Enable multicast loopback (for local testing)
    socket.set_multicast_loop_v4(true)?;

    // Set non-blocking
    socket.set_nonblocking(true)?;

    Ok(socket.into())
}

/// Receiver task - listens for discovery messages.
///
/// # Arguments
/// * `receiver_type` - "topics" (port 10317) or "services" (port 10318) for logging
async fn receiver_task(
    socket: Arc<tokio::net::UdpSocket>,
    registry: SharedRegistry,
    running: Arc<AtomicBool>,
    our_partition: String,
    receiver_type: &'static str,
) {
    debug!(
        "{} receiver task started for partition '{}'",
        receiver_type, our_partition
    );
    let mut buf = vec![0u8; 65535];

    while running.load(Ordering::Relaxed) {
        match tokio::time::timeout(Duration::from_millis(250), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                trace!(
                    "[GZ-DISCOVERY] [{}] Received {} bytes from {}",
                    receiver_type, len, addr
                );
                if let Err(e) = handle_discovery_message(&buf[..len], &registry, &our_partition) {
                    trace!(
                        "[GZ-DISCOVERY] [{}] Failed to handle message: {}",
                        receiver_type, e
                    );
                }
            }
            Ok(Err(e)) => {
                if e.kind() != std::io::ErrorKind::WouldBlock {
                    warn!("[GZ-DISCOVERY] [{}] recv error: {}", receiver_type, e);
                }
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }

    debug!("[GZ-DISCOVERY] {} receiver task stopped", receiver_type);
}

/// Handle a received discovery message.
fn handle_discovery_message(data: &[u8], registry: &Registry, our_partition: &str) -> Result<()> {
    let body = decode_frame(data)?;
    let msg = Discovery::decode(body)?;

    debug!(
        "Discovery message type={:?} from process={}",
        msg.r#type(),
        &msg.process_uuid[..8.min(msg.process_uuid.len())]
    );

    // Check partition match
    if let Some(discovery::DiscContents::Pub(publisher)) = &msg.disc_contents {
        debug!(
            "ADVERTISE topic='{}' addr='{}'",
            publisher.topic, publisher.address
        );
        // Extract partition from topic
        if let Some((partition, _topic)) = crate::partition::parse_fully_qualified(&publisher.topic)
        {
            if partition != our_partition {
                debug!(
                    "Partition mismatch: topic has '{}', we want '{}'",
                    partition, our_partition
                );
                return Ok(());
            }
            debug!("Partition matches: '{}'", partition);
        } else {
            debug!("Could not parse partition from topic");
        }
    }

    match msg.r#type() {
        discovery::Type::Advertise => {
            if let Some(discovery::DiscContents::Pub(publisher)) = msg.disc_contents {
                handle_advertise(registry, &publisher, &msg.process_uuid);
            }
        }
        discovery::Type::Unadvertise => {
            if let Some(discovery::DiscContents::Pub(publisher)) = msg.disc_contents {
                handle_unadvertise(registry, &publisher);
            }
        }
        discovery::Type::Bye => {
            registry.remove_process(&msg.process_uuid);
            debug!("Process {} disconnected", msg.process_uuid);
        }
        discovery::Type::Heartbeat => {
            // Heartbeat just keeps process alive - already handled by timestamps
            trace!("Heartbeat from {}", msg.process_uuid);
        }
        _ => {
            trace!("Ignoring discovery message type: {:?}", msg.r#type());
        }
    }

    Ok(())
}

/// Handle an ADVERTISE message.
fn handle_advertise(registry: &Registry, publisher: &discovery::Publisher, process_uuid: &str) {
    let fqn = &publisher.topic;
    let address = &publisher.address;
    let node_uuid = &publisher.node_uuid;

    // Extract plain topic name from FQN (e.g., "@/partition@/world/default/clock" -> "/world/default/clock")
    let topic = crate::partition::parse_fully_qualified(fqn)
        .map(|(_, t)| t)
        .unwrap_or_else(|| fqn.clone());

    match &publisher.pub_type {
        Some(discovery::publisher::PubType::MsgPub(msg_pub)) => {
            debug!(
                "Discovered topic: {} ({}) at {} (ctrl: {})",
                topic, msg_pub.msg_type, address, msg_pub.ctrl
            );
            registry.add_publisher(
                &topic,
                &msg_pub.msg_type,
                address,
                &msg_pub.ctrl,
                node_uuid,
                process_uuid,
            );
        }
        Some(discovery::publisher::PubType::SrvPub(srv_pub)) => {
            debug!(
                "Discovered service: {} ({} -> {}) at {}",
                topic, srv_pub.request_type, srv_pub.response_type, address
            );
            registry.add_service(
                &topic,
                &srv_pub.request_type,
                &srv_pub.response_type,
                &srv_pub.socket_id,
                address,
                node_uuid,
                process_uuid,
            );
        }
        None => {
            warn!("Advertise message without publisher type");
        }
    }
}

/// Handle an UNADVERTISE message.
fn handle_unadvertise(registry: &Registry, publisher: &discovery::Publisher) {
    let fqn = &publisher.topic;
    let node_uuid = &publisher.node_uuid;

    // Extract plain topic name from FQN
    let topic = crate::partition::parse_fully_qualified(fqn)
        .map(|(_, t)| t)
        .unwrap_or_else(|| fqn.clone());

    match &publisher.pub_type {
        Some(discovery::publisher::PubType::MsgPub(_)) => {
            debug!("Removed topic publisher: {} from {}", topic, node_uuid);
            registry.remove_publisher(&topic, node_uuid);
        }
        Some(discovery::publisher::PubType::SrvPub(_)) => {
            debug!("Removed service: {}", topic);
            registry.remove_service(&topic);
        }
        None => {}
    }
}

/// Heartbeat task - sends periodic heartbeats.
async fn heartbeat_task(
    socket: Arc<tokio::net::UdpSocket>,
    running: Arc<AtomicBool>,
    interval: Duration,
    process_uuid: String,
    multicast_addr: SocketAddrV4,
) {
    let mut ticker = tokio::time::interval(interval);

    while running.load(Ordering::Relaxed) {
        ticker.tick().await;

        let msg = Discovery {
            header: None,
            version: WIRE_VERSION,
            process_uuid: process_uuid.clone(),
            r#type: discovery::Type::Heartbeat as i32,
            flags: None,
            disc_contents: None,
        };

        let frame = match encode_frame(&msg) {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to encode heartbeat: {}", e);
                continue;
            }
        };

        if let Err(e) = socket.send_to(&frame, multicast_addr).await {
            warn!("Failed to send heartbeat: {}", e);
        }
    }

    // Send BYE on shutdown
    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid,
        r#type: discovery::Type::Bye as i32,
        flags: None,
        disc_contents: None,
    };

    match encode_frame(&msg) {
        Ok(frame) => {
            let _ = socket.send_to(&frame, multicast_addr).await;
        }
        Err(e) => {
            warn!("Failed to encode BYE message: {}", e);
        }
    }

    debug!("Heartbeat task stopped");
}

/// Cleanup task - removes stale entries.
async fn cleanup_task(registry: SharedRegistry, running: Arc<AtomicBool>) {
    let cleanup_interval = Duration::from_millis(SILENCE_TIMEOUT_MS / 2);
    let max_age = Duration::from_millis(SILENCE_TIMEOUT_MS);

    let mut ticker = tokio::time::interval(cleanup_interval);

    while running.load(Ordering::Relaxed) {
        ticker.tick().await;
        registry.cleanup_stale(max_age);
    }

    debug!("Cleanup task stopped");
}

/// Discovery sender task - sends SUBSCRIBE and NEW_CONNECTION requests.
async fn discovery_sender_task(
    socket: Arc<tokio::net::UdpSocket>,
    mut rx: mpsc::Receiver<DiscoveryRequest>,
    running: Arc<AtomicBool>,
    process_uuid: String,
    partition: String,
    multicast_addr: SocketAddrV4,
) {
    debug!(
        "Sender task started for partition '{}', multicast: {}",
        partition, multicast_addr
    );

    while running.load(Ordering::Relaxed) {
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Some(request)) => {
                let (msg_type, disc_contents, fq_topic, log_prefix) = match request {
                    DiscoveryRequest::Subscribe(topic) => {
                        let fq = fully_qualified_name(&partition, "", &topic);
                        debug!("Sending SUBSCRIBE for '{}' (fq: {})", topic, fq);
                        let contents = discovery::DiscContents::Sub(discovery::Subscriber {
                            topic: fq.clone(),
                        });
                        (discovery::Type::Subscribe, contents, fq, "subscribe")
                    }
                    DiscoveryRequest::NewConnection {
                        topic,
                        publisher_ctrl,
                    } => {
                        let fq = fully_qualified_name(&partition, "", &topic);
                        debug!(
                            "Sending NEW_CONNECTION for '{}' (fq: {}, ctrl: {})",
                            topic, fq, publisher_ctrl
                        );
                        // NEW_CONNECTION uses Publisher message with ctrl set to publisher's pUUID
                        // This is how the publisher knows the message is addressed to them
                        //
                        // CRITICAL: msg_type must be set to "google.protobuf.Message" to act as
                        // a wildcard subscriber. The publisher's HasTopic() checks:
                        //   _pub.MsgTypeName() == _type || _pub.MsgTypeName() == kGenericMessageType
                        // where kGenericMessageType = "google.protobuf.Message"
                        // Empty string matches neither, causing HasConnections() to return false!
                        let contents = discovery::DiscContents::Pub(discovery::Publisher {
                            topic: fq.clone(),
                            address: String::new(), // We're the subscriber, not providing an address
                            process_uuid: process_uuid.clone(),
                            node_uuid: process_uuid.clone(), // Use process UUID as node UUID
                            scope: discovery::publisher::Scope::All as i32,
                            pub_type: Some(discovery::publisher::PubType::MsgPub(
                                discovery::MessagePublisher {
                                    ctrl: publisher_ctrl, // Publisher's pUUID - key for routing!
                                    msg_type: "google.protobuf.Message".to_string(), // Wildcard type
                                    throttled: false,
                                    msgs_per_sec: 0,
                                },
                            )),
                        });
                        (
                            discovery::Type::NewConnection,
                            contents,
                            fq,
                            "new_connection",
                        )
                    }
                    DiscoveryRequest::Advertise {
                        topic,
                        msg_type,
                        address,
                    } => {
                        let fq = fully_qualified_name(&partition, "", &topic);
                        debug!(
                            "Sending ADVERTISE for '{}' (fq: {}, addr: {})",
                            topic, fq, address
                        );
                        let contents = discovery::DiscContents::Pub(discovery::Publisher {
                            topic: fq.clone(),
                            address,
                            process_uuid: process_uuid.clone(),
                            node_uuid: process_uuid.clone(),
                            scope: discovery::publisher::Scope::All as i32,
                            pub_type: Some(discovery::publisher::PubType::MsgPub(
                                discovery::MessagePublisher {
                                    ctrl: process_uuid.clone(),
                                    msg_type,
                                    throttled: false,
                                    msgs_per_sec: 0,
                                },
                            )),
                        });
                        (discovery::Type::Advertise, contents, fq, "advertise")
                    }
                };

                let msg = Discovery {
                    header: None,
                    version: WIRE_VERSION,
                    process_uuid: process_uuid.clone(),
                    r#type: msg_type as i32,
                    flags: None,
                    disc_contents: Some(disc_contents),
                };

                let frame = match encode_frame(&msg) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("Failed to encode discovery message: {}", e);
                        continue;
                    }
                };

                if let Err(e) = socket.send_to(&frame, multicast_addr).await {
                    warn!("Failed to send {}: {}", log_prefix, e);
                } else {
                    debug!(
                        "{} sent to {} (fq: {})",
                        log_prefix.to_uppercase(),
                        multicast_addr,
                        fq_topic
                    );
                }
            }
            Ok(None) => {
                // Channel closed
                break;
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }

    debug!("Discovery sender task stopped");
}
