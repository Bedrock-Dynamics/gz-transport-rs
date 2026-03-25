//! Discovery protocol implementation.
//!
//! Handles UDP multicast discovery for finding Gazebo topics and services.

mod client;
mod message;
mod registry;

pub use client::DiscoveryClient;
pub use registry::{PublisherInfo, ServiceInfo, TopicInfo};
