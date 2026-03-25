//! Error types for gz-transport.

use std::time::Duration;
use thiserror::Error;

/// Error type for gz-transport operations.
#[derive(Error, Debug)]
pub enum Error {
    // Discovery errors
    /// Topic discovery timed out.
    #[error("Discovery timeout: no response for topic '{topic}'")]
    DiscoveryTimeout {
        /// The topic that was not found.
        topic: String,
    },

    /// Topic not found in discovery.
    #[error("Topic not found: {0}")]
    TopicNotFound(String),

    /// Service not found in discovery.
    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    /// No publishers available for topic.
    #[error("No publishers for topic: {0}")]
    NoPublishers(String),

    // Transport errors
    /// Connection to endpoint failed.
    #[error("Connection failed to {address}: {message}")]
    ConnectionFailed {
        /// The address that failed.
        address: String,
        /// Error description.
        message: String,
    },

    /// ZeroMQ error.
    #[error("ZeroMQ error: {0}")]
    Zmq(#[from] zmq::Error),

    /// Socket closed unexpectedly.
    #[error("Socket disconnected")]
    Disconnected,

    /// Failed to bind socket.
    #[error("Failed to bind socket: {0}")]
    BindFailed(String),

    // Protocol errors
    /// Invalid frame count in message.
    #[error("Invalid frame: expected {expected} frames, got {actual}")]
    InvalidFrame {
        /// Expected frame count.
        expected: usize,
        /// Actual frame count.
        actual: usize,
    },

    /// Message too large for frame encoding.
    #[error("Message too large: {size} bytes exceeds maximum of {max} bytes")]
    MessageTooLarge {
        /// Actual message size.
        size: usize,
        /// Maximum allowed size.
        max: usize,
    },

    /// Protobuf decode error.
    #[error("Protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    /// Protobuf encode error.
    #[error("Protobuf encode error: {0}")]
    Encode(#[from] prost::EncodeError),

    /// Invalid topic name.
    #[error("Invalid topic name: {reason}")]
    InvalidTopicName {
        /// Why the topic name is invalid.
        reason: String,
    },

    /// Invalid partition name.
    #[error("Invalid partition name: {reason}")]
    InvalidPartition {
        /// Why the partition name is invalid.
        reason: String,
    },

    // Service errors
    /// Service call returned failure.
    #[error("Service call failed: {0}")]
    ServiceCallFailed(String),

    /// Service call timed out.
    #[error("Service timeout after {0:?}")]
    ServiceTimeout(Duration),

    /// Service response correlation failed.
    #[error("Service response UUID mismatch")]
    ResponseMismatch,

    // IO errors
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Channel send error.
    #[error("Channel closed")]
    ChannelClosed,
}

/// Result type for gz-transport operations.
pub type Result<T> = std::result::Result<T, Error>;
