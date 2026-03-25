//! # gz-transport
//!
//! Pure Rust implementation of the Gazebo Transport protocol.
//!
//! This crate provides a native Rust client for communicating with Gazebo Sim
//! without requiring any C++ dependencies. It implements the gz-transport
//! protocol directly, supporting topic subscription and service calls.
//!
//! ## Features
//!
//! - **No Gazebo C++ stack required** - Only depends on libzmq
//! - **Version-agnostic** - Works with Gazebo Harmonic, Ionic, Jetty, and beyond
//! - **Async-first** - Built on Tokio for efficient async I/O
//! - **Cross-platform** - Linux, macOS, Windows
//! - **Generic API** - Works with any Gazebo simulation (drones, robots, etc.)
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use gz_transport::{Node, Result};
//! use gz_transport::msgs::PoseV;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create a node (auto-discovers Gazebo)
//!     let mut node = Node::new(None).await?;
//!
//!     // List discovered topics
//!     for topic in node.topics() {
//!         println!("Found topic: {} ({})", topic.topic, topic.msg_type);
//!     }
//!
//!     // Subscribe to poses
//!     let mut sub = node.subscribe::<PoseV>("/world/default/pose/info").await?;
//!
//!     // Receive messages
//!     while let Some((msg, _meta)) = sub.recv().await {
//!         println!("Received {} poses", msg.pose.len());
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Simulation Control
//!
//! ```rust,no_run
//! use gz_transport::{Node, Result};
//! use gz_transport::msgs::{WorldControl, Boolean};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let mut node = Node::new(None).await?;
//!
//!     // Pause the simulation
//!     let request = WorldControl {
//!         pause: true,
//!         ..Default::default()
//!     };
//!
//!     let response: Boolean = node
//!         .call_service("/world/default/control", request)
//!         .await?;
//!
//!     println!("Paused: {}", response.data);
//!     Ok(())
//! }
//! ```
//!
//! ## Configuration
//!
//! The node can be configured via environment variables or programmatically:
//!
//! - `GZ_PARTITION` - Partition name for topic filtering
//! - `GZ_DISCOVERY_MULTICAST_IP` - Multicast address (default: 239.255.0.7)
//! - `GZ_DISCOVERY_MSG_PORT` - Message discovery port (default: 10317)
//! - `GZ_IP` - Override host IP address
//!
//! ```rust,no_run
//! use gz_transport::{Node, Config};
//! use std::time::Duration;
//!
//! # async fn example() -> gz_transport::Result<()> {
//! let config = Config::builder()
//!     .partition(Some("my_partition"))
//!     .discovery_timeout(Duration::from_secs(10))
//!     .build();
//!
//! let node = Node::with_config(config).await?;
//! # Ok(())
//! # }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(unsafe_code)]
#![warn(missing_docs)]

// Public modules
pub mod config;
pub mod error;
pub mod msgs;

// Private implementation
mod discovery;
mod node;
mod partition;
mod transport;

// Public re-exports
pub use config::{Config, ConfigBuilder};
pub use error::{Error, Result};
pub use node::Node;
pub use transport::{Publisher, ServiceClient, Subscriber};

// Re-export discovery types that users might need
pub use discovery::{PublisherInfo, ServiceInfo, TopicInfo};

/// Prelude for convenient imports.
///
/// ```rust
/// use gz_transport::prelude::*;
/// ```
pub mod prelude {
    pub use crate::config::Config;
    pub use crate::error::{Error, Result};
    pub use crate::node::Node;
    pub use crate::transport::{Publisher, ServiceClient, Subscriber};
    pub use crate::{PublisherInfo, ServiceInfo, TopicInfo};
}
