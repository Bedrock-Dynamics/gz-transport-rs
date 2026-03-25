//! ZeroMQ subscriber implementation using libzmq bindings.

use crate::error::{Error, Result};
use prost::Message;
use std::marker::PhantomData;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

/// Metadata about a received message.
#[derive(Debug, Clone)]
pub struct MessageMeta {
    /// Topic name.
    pub topic: String,
    /// Sender address.
    pub sender: String,
    /// Message type name (e.g., "gz.msgs.Pose_V").
    pub msg_type: String,
}

/// A subscriber for a Gazebo topic.
///
/// Receives messages of type `T` from the topic.
pub struct Subscriber<T> {
    /// Channel for receiving messages
    rx: mpsc::Receiver<(T, MessageMeta)>,
    /// Handle to the background thread
    _handle: std::thread::JoinHandle<()>,
    /// Type marker
    _phantom: PhantomData<T>,
}

impl<T> Subscriber<T>
where
    T: Message + Default + Send + 'static,
{
    /// Connect to a publisher endpoint.
    pub async fn connect(endpoint: &str, topic: &str) -> Result<Self> {
        Self::connect_all(&[endpoint.to_string()], topic).await
    }

    /// Connect to multiple publisher endpoints.
    /// This is needed when a topic has multiple publishers (common in Gazebo).
    pub async fn connect_all(endpoints: &[String], topic: &str) -> Result<Self> {
        if endpoints.is_empty() {
            return Err(Error::NoPublishers(topic.to_string()));
        }

        let endpoints = endpoints.to_vec();
        let topic = topic.to_string();

        // Create channel for messages
        let (tx, rx) = mpsc::channel(100);

        // Spawn receiver thread (zmq is synchronous)
        let handle = std::thread::spawn(move || {
            if let Err(e) = subscriber_thread_multi::<T>(&endpoints, &topic, tx) {
                warn!("[ZMQ-SUB] Subscriber thread error: {}", e);
            }
        });

        // Give the thread time to connect and subscribe
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        Ok(Self {
            rx,
            _handle: handle,
            _phantom: PhantomData,
        })
    }

    /// Receive the next message.
    ///
    /// Returns `None` if the subscriber is closed.
    pub async fn recv(&mut self) -> Option<(T, MessageMeta)> {
        self.rx.recv().await
    }

    /// Try to receive a message without blocking.
    pub fn try_recv(&mut self) -> Option<(T, MessageMeta)> {
        self.rx.try_recv().ok()
    }
}

/// Background thread for receiving messages using libzmq.
/// Connects to multiple endpoints (ZMQ SUB can connect to multiple PUBs).
fn subscriber_thread_multi<T>(
    endpoints: &[String],
    topic: &str,
    tx: mpsc::Sender<(T, MessageMeta)>,
) -> Result<()>
where
    T: Message + Default + Send + 'static,
{
    trace!(
        "[ZMQ-SUB] Creating context and socket for '{}' with {} endpoints",
        topic,
        endpoints.len()
    );

    // Create ZMQ context
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::SUB).map_err(|e| Error::ConnectionFailed {
        address: endpoints.join(", "),
        message: format!("Failed to create SUB socket: {}", e),
    })?;

    // Set socket options
    socket
        .set_rcvtimeo(5000)
        .map_err(|e| warn!("set_rcvtimeo on SUB socket: {}", e))
        .ok();
    socket
        .set_linger(0)
        .map_err(|e| warn!("set_linger on SUB socket: {}", e))
        .ok();

    // Connect to ALL endpoints (ZMQ SUB supports multiple connections)
    for endpoint in endpoints {
        socket
            .connect(endpoint)
            .map_err(|e| Error::ConnectionFailed {
                address: endpoint.to_string(),
                message: format!("Failed to connect: {}", e),
            })?;
        trace!("[ZMQ-SUB] Connected to {}", endpoint);
    }

    // Subscribe using fully qualified topic name as filter
    // Gazebo expects FQN format: @partition@/topic
    socket
        .set_subscribe(topic.as_bytes())
        .map_err(|e| Error::ConnectionFailed {
            address: endpoints.join(", "),
            message: format!("Failed to subscribe: {}", e),
        })?;

    trace!(
        "[ZMQ-SUB] Subscribed with filter '{}' to {} endpoints",
        topic,
        endpoints.len()
    );
    debug!(
        "Connected to {} endpoints for topic '{}'",
        endpoints.len(),
        topic
    );

    // Wait for subscription to propagate (slow joiner problem)
    std::thread::sleep(std::time::Duration::from_millis(200));
    debug!("Subscription propagated, starting receiver");

    loop {
        // Receive multipart message
        let msg = match socket.recv_multipart(0) {
            Ok(frames) => frames,
            Err(zmq::Error::EAGAIN) => {
                trace!("[ZMQ-SUB] Recv timeout (5s) - no messages, still waiting...");
                continue;
            }
            Err(e) => {
                warn!("[ZMQ-SUB] Recv error: {}", e);
                break;
            }
        };

        trace!("[ZMQ-SUB] Received message with {} frames", msg.len());

        // Gazebo message format:
        // Frame 0: Topic
        // Frame 1: Sender address
        // Frame 2: Message data
        // Frame 3: Message type
        // Frame 4: (optional) Statistics

        if msg.len() < 4 {
            warn!(
                "[ZMQ-SUB] Invalid message: expected at least 4 frames, got {}",
                msg.len()
            );
            continue;
        }

        let topic_frame = String::from_utf8_lossy(&msg[0]).to_string();
        let sender = String::from_utf8_lossy(&msg[1]).to_string();
        let data = &msg[2];
        let msg_type = String::from_utf8_lossy(&msg[3]).to_string();

        trace!(
            "[ZMQ-SUB] Frame details: topic='{}' sender='{}' msg_type='{}' data_len={}",
            topic_frame,
            sender,
            msg_type,
            data.len()
        );

        let meta = MessageMeta {
            topic: topic_frame,
            sender,
            msg_type: msg_type.clone(),
        };

        match T::decode(data.as_slice()) {
            Ok(decoded) => {
                debug!("[ZMQ-SUB] Decoded {} successfully", msg_type);
                // Use blocking send since we're in a std thread
                if tx.blocking_send((decoded, meta)).is_err() {
                    debug!("Channel closed, stopping subscriber");
                    break;
                }
            }
            Err(e) => {
                warn!("[ZMQ-SUB] Failed to decode {}: {}", msg_type, e);
            }
        }
    }

    debug!("Subscriber thread stopped");
    Ok(())
}
