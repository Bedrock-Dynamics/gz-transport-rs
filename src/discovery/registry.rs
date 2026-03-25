//! Registry for discovered topics and services.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Information about a discovered topic.
#[derive(Debug, Clone)]
pub struct TopicInfo {
    /// Topic name.
    pub topic: String,
    /// Message type name (e.g., "gz.msgs.Pose_V").
    pub msg_type: String,
    /// Publisher information.
    pub publishers: Vec<PublisherInfo>,
}

/// Information about a publisher.
#[derive(Debug, Clone)]
pub struct PublisherInfo {
    /// ZMQ TCP address (e.g., "tcp://192.168.1.100:45678").
    pub address: String,
    /// ZMQ control address (for NEW_CONNECTION messages).
    pub ctrl_address: String,
    /// Node UUID.
    pub node_uuid: String,
    /// Process UUID.
    pub process_uuid: String,
    /// Last seen time.
    pub last_seen: Instant,
}

/// Information about a discovered service.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Service name.
    pub service: String,
    /// Request type name.
    pub request_type: String,
    /// Response type name.
    pub response_type: String,
    /// ZMQ socket ID.
    pub socket_id: String,
    /// ZMQ TCP address.
    pub address: String,
    /// Node UUID.
    pub node_uuid: String,
    /// Process UUID.
    pub process_uuid: String,
    /// Last seen time.
    pub last_seen: Instant,
}

/// Registry entry for a topic.
#[derive(Debug)]
struct TopicEntry {
    msg_type: String,
    publishers: HashMap<String, PublisherInfo>, // node_uuid -> info
}

/// Registry entry for a service.
#[derive(Debug)]
struct ServiceEntry {
    info: ServiceInfo,
}

/// Thread-safe registry of discovered topics and services.
#[derive(Debug, Default)]
pub struct Registry {
    topics: RwLock<HashMap<String, TopicEntry>>,
    services: RwLock<HashMap<String, ServiceEntry>>,
}

impl Registry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a topic publisher.
    pub fn add_publisher(
        &self,
        topic: &str,
        msg_type: &str,
        address: &str,
        ctrl_address: &str,
        node_uuid: &str,
        process_uuid: &str,
    ) {
        let mut topics = self.topics.write();

        let entry = topics
            .entry(topic.to_string())
            .or_insert_with(|| TopicEntry {
                msg_type: msg_type.to_string(),
                publishers: HashMap::new(),
            });

        entry.publishers.insert(
            node_uuid.to_string(),
            PublisherInfo {
                address: address.to_string(),
                ctrl_address: ctrl_address.to_string(),
                node_uuid: node_uuid.to_string(),
                process_uuid: process_uuid.to_string(),
                last_seen: Instant::now(),
            },
        );
    }

    /// Remove a publisher by node UUID.
    pub fn remove_publisher(&self, topic: &str, node_uuid: &str) {
        let mut topics = self.topics.write();

        if let Some(entry) = topics.get_mut(topic) {
            entry.publishers.remove(node_uuid);

            // Remove topic if no publishers left
            if entry.publishers.is_empty() {
                topics.remove(topic);
            }
        }
    }

    /// Remove all publishers for a process.
    pub fn remove_process(&self, process_uuid: &str) {
        let mut topics = self.topics.write();

        for entry in topics.values_mut() {
            entry
                .publishers
                .retain(|_, p| p.process_uuid != process_uuid);
        }

        // Remove empty topics
        topics.retain(|_, e| !e.publishers.is_empty());

        // Also remove services for this process
        let mut services = self.services.write();
        services.retain(|_, e| e.info.process_uuid != process_uuid);
    }

    /// Add or update a service.
    #[allow(clippy::too_many_arguments)] // Service metadata requires all these fields
    pub fn add_service(
        &self,
        service: &str,
        request_type: &str,
        response_type: &str,
        socket_id: &str,
        address: &str,
        node_uuid: &str,
        process_uuid: &str,
    ) {
        let mut services = self.services.write();

        services.insert(
            service.to_string(),
            ServiceEntry {
                info: ServiceInfo {
                    service: service.to_string(),
                    request_type: request_type.to_string(),
                    response_type: response_type.to_string(),
                    socket_id: socket_id.to_string(),
                    address: address.to_string(),
                    node_uuid: node_uuid.to_string(),
                    process_uuid: process_uuid.to_string(),
                    last_seen: Instant::now(),
                },
            },
        );
    }

    /// Remove a service.
    pub fn remove_service(&self, service: &str) {
        let mut services = self.services.write();
        services.remove(service);
    }

    /// Get topic info.
    pub fn get_topic(&self, topic: &str) -> Option<TopicInfo> {
        let topics = self.topics.read();

        topics.get(topic).map(|entry| TopicInfo {
            topic: topic.to_string(),
            msg_type: entry.msg_type.clone(),
            publishers: entry.publishers.values().cloned().collect(),
        })
    }

    /// Get service info.
    pub fn get_service(&self, service: &str) -> Option<ServiceInfo> {
        let services = self.services.read();
        services.get(service).map(|entry| entry.info.clone())
    }

    /// List all topics.
    pub fn list_topics(&self) -> Vec<TopicInfo> {
        let topics = self.topics.read();

        topics
            .iter()
            .map(|(topic, entry)| TopicInfo {
                topic: topic.clone(),
                msg_type: entry.msg_type.clone(),
                publishers: entry.publishers.values().cloned().collect(),
            })
            .collect()
    }

    /// List all services.
    pub fn list_services(&self) -> Vec<ServiceInfo> {
        let services = self.services.read();
        services.values().map(|entry| entry.info.clone()).collect()
    }

    /// Remove stale entries older than the given duration.
    pub fn cleanup_stale(&self, max_age: std::time::Duration) {
        let now = Instant::now();

        let mut topics = self.topics.write();
        for entry in topics.values_mut() {
            entry
                .publishers
                .retain(|_, p| now.duration_since(p.last_seen) < max_age);
        }
        topics.retain(|_, e| !e.publishers.is_empty());

        let mut services = self.services.write();
        services.retain(|_, e| now.duration_since(e.info.last_seen) < max_age);
    }
}

/// Shared registry handle.
pub type SharedRegistry = Arc<Registry>;
