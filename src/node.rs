//! High-level Node API for Gazebo Transport.

use crate::config::Config;
use crate::discovery::{DiscoveryClient, ServiceInfo, TopicInfo};
use crate::error::{Error, Result};
use crate::transport::{ServiceClient, Subscriber};
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::{debug, info};

/// A Gazebo Transport node.
///
/// Represents a participant in the Gazebo Transport network. Handles
/// discovery, topic subscription, and service calls.
///
/// # Thread Safety
///
/// Node is `Send` but not `Sync`. For shared access, wrap in `Arc<Mutex<Node>>`.
///
/// # Example
///
/// ```rust,no_run
/// use gz_transport::{Node, Result};
/// use gz_transport::msgs::PoseV;
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     // Create a node
///     let mut node = Node::new(None).await?;
///
///     // Subscribe to poses
///     let mut sub = node.subscribe::<PoseV>("/world/default/pose/info").await?;
///
///     // Receive messages
///     while let Some((msg, _meta)) = sub.recv().await {
///         println!("Received {} poses", msg.pose.len());
///     }
///
///     Ok(())
/// }
/// ```
pub struct Node {
    config: Config,
    discovery: DiscoveryClient,
    service_clients: HashMap<String, ServiceClient>,
}

impl Node {
    /// Create a new node with optional partition.
    ///
    /// If `partition` is `None`, uses the default partition from
    /// `GZ_PARTITION` environment variable, or `<hostname>:<username>`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if unable to bind UDP socket.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gz_transport::Node;
    ///
    /// # async fn example() -> gz_transport::Result<()> {
    /// // Use default partition
    /// let node = Node::new(None).await?;
    ///
    /// // Use custom partition
    /// let node = Node::new(Some("my_partition")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(partition: Option<&str>) -> Result<Self> {
        let config = Config::builder().partition(partition).build();
        Self::with_config(config).await
    }

    /// Create a node with custom configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gz_transport::{Node, Config};
    /// use std::time::Duration;
    ///
    /// # async fn example() -> gz_transport::Result<()> {
    /// let config = Config::builder()
    ///     .partition(Some("my_partition"))
    ///     .discovery_timeout(Duration::from_secs(10))
    ///     .build();
    ///
    /// let node = Node::with_config(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn with_config(config: Config) -> Result<Self> {
        let mut discovery = DiscoveryClient::new(config.clone());
        discovery.start().await?;

        info!(
            "Node started with partition: {}",
            config.effective_partition()
        );

        Ok(Self {
            config,
            discovery,
            service_clients: HashMap::new(),
        })
    }

    /// Subscribe to a topic.
    ///
    /// Automatically discovers the topic publisher and connects.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The protobuf message type (e.g., `gz_transport::msgs::PoseV`)
    ///
    /// # Errors
    ///
    /// Returns [`Error::DiscoveryTimeout`] if topic not found within timeout.
    /// Returns [`Error::NoPublishers`] if topic has no publishers.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gz_transport::{Node, msgs::PoseV};
    ///
    /// # async fn example() -> gz_transport::Result<()> {
    /// let mut node = Node::new(None).await?;
    /// let mut sub = node.subscribe::<PoseV>("/world/default/pose/info").await?;
    ///
    /// while let Some((msg, meta)) = sub.recv().await {
    ///     println!("Topic: {}, Poses: {}", meta.topic, msg.pose.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe<T>(&mut self, topic: &str) -> Result<Subscriber<T>>
    where
        T: Message + Default + Send + 'static,
    {
        // Discover topic
        let info = self
            .discovery
            .get_topic(topic, self.config.discovery_timeout)
            .await?;

        if info.publishers.is_empty() {
            return Err(Error::NoPublishers(topic.to_string()));
        }

        // Build fully qualified topic name for ZMQ subscription filter
        // Format: @partition@/topic (Gazebo expects this as the filter)
        let partition = self.config.effective_partition();
        let fqn = crate::partition::fully_qualified_name(&partition, "", topic);

        // Collect all unique publisher addresses
        let addresses: Vec<String> = info
            .publishers
            .iter()
            .map(|p| p.address.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        debug!(
            "Connecting to {} publishers for '{}' (fqn: '{}'): {:?}",
            addresses.len(),
            topic,
            fqn,
            addresses
        );

        // Create subscriber connecting to ALL publishers
        // (ZMQ SUB socket can connect to multiple PUB sockets)
        let subscriber = Subscriber::connect_all(&addresses, &fqn).await?;

        // Send NEW_CONNECTION to ALL publishers to notify them we're a remote subscriber.
        // This is CRITICAL - without this, gz-transport publishers won't send data
        // because they skip serialization when remoteSubscribers is empty.
        for publisher in &info.publishers {
            debug!(
                "Sending NEW_CONNECTION to publisher {} at {}",
                publisher.process_uuid, publisher.address
            );
            self.discovery
                .notify_connection(topic, &publisher.process_uuid)
                .await?;
        }

        Ok(subscriber)
    }

    /// Call a service.
    ///
    /// Automatically discovers the service endpoint and connects.
    ///
    /// # Type Parameters
    ///
    /// * `Req` - Request message type
    /// * `Res` - Response message type
    ///
    /// # Errors
    ///
    /// Returns [`Error::ServiceNotFound`] if service not found.
    /// Returns [`Error::ServiceTimeout`] if service doesn't respond.
    /// Returns [`Error::ServiceCallFailed`] if service returns failure.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gz_transport::{Node, msgs::{WorldControl, Boolean}};
    ///
    /// # async fn example() -> gz_transport::Result<()> {
    /// let mut node = Node::new(None).await?;
    ///
    /// // Pause the simulation
    /// let request = WorldControl {
    ///     pause: true,
    ///     ..Default::default()
    /// };
    ///
    /// let response: Boolean = node
    ///     .call_service("/world/default/control", request)
    ///     .await?;
    ///
    /// println!("Success: {}", response.data);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_service<Req, Res>(&mut self, service: &str, request: Req) -> Result<Res>
    where
        Req: Message,
        Res: Message + Default,
    {
        self.call_service_with_timeout(service, request, self.config.service_timeout)
            .await
    }

    /// Call a service with custom timeout.
    pub async fn call_service_with_timeout<Req, Res>(
        &mut self,
        service: &str,
        request: Req,
        timeout: Duration,
    ) -> Result<Res>
    where
        Req: Message,
        Res: Message + Default,
    {
        // Get or create service client
        let client = self.get_or_create_service_client(service).await?;

        // Make the call
        client.call(request, timeout).await
    }

    /// Get or create a service client.
    async fn get_or_create_service_client(&mut self, service: &str) -> Result<&ServiceClient> {
        if !self.service_clients.contains_key(service) {
            // Discover service
            let info = self
                .discovery
                .get_service(service, self.config.discovery_timeout)
                .await?;

            debug!(
                "Connecting to service '{}' at {} (socket_id: {})",
                service, info.address, info.socket_id
            );

            // Create client, passing partition, host_ip and response_port from config
            let partition = self.config.effective_partition();
            let client = ServiceClient::connect(
                &info.address,
                service,
                &partition,
                &info.socket_id,
                self.config.host_ip,
                self.config.service_response_port,
            )
            .await?;
            self.service_clients.insert(service.to_string(), client);
        }

        // SAFETY: we just inserted the key above if it was missing
        self.service_clients
            .get(service)
            .ok_or_else(|| Error::ServiceNotFound(service.to_string()))
    }

    /// List all discovered topics.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gz_transport::Node;
    ///
    /// # async fn example() -> gz_transport::Result<()> {
    /// let node = Node::new(None).await?;
    ///
    /// for topic in node.topics() {
    ///     println!("{} ({})", topic.topic, topic.msg_type);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn topics(&self) -> Vec<TopicInfo> {
        self.discovery.topics()
    }

    /// List all discovered services.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gz_transport::Node;
    ///
    /// # async fn example() -> gz_transport::Result<()> {
    /// let node = Node::new(None).await?;
    ///
    /// for svc in node.services() {
    ///     println!("{} ({} -> {})", svc.service, svc.request_type, svc.response_type);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn services(&self) -> Vec<ServiceInfo> {
        self.discovery.services()
    }

    /// Create a publisher for the given topic.
    ///
    /// Binds a ZMQ PUB socket, registers with the discovery network via
    /// ADVERTISE, and returns a [`Publisher`] handle. Subscribers on the
    /// Gazebo network will discover this publisher and connect automatically.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The protobuf message type (e.g., `gz_transport::msgs::Double`)
    ///
    /// # Errors
    ///
    /// Returns [`Error::BindFailed`] if the ZMQ socket cannot bind.
    pub async fn advertise<T>(
        &mut self,
        topic: &str,
        msg_type: &str,
    ) -> Result<crate::transport::publisher::Publisher<T>>
    where
        T: Message + Default + Send + 'static,
    {
        let publisher = crate::transport::publisher::Publisher::bind(topic, msg_type)?;

        self.discovery
            .advertise(topic, msg_type, publisher.address())
            .await?;

        info!(
            topic,
            msg_type,
            address = publisher.address(),
            "advertised publisher"
        );

        Ok(publisher)
    }

    /// Get the node's partition.
    pub fn partition(&self) -> String {
        self.config.effective_partition()
    }

    /// Get the node's configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.discovery.stop();
        debug!("Node stopped");
    }
}
