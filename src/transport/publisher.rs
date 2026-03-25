//! ZMQ PUB socket publisher for Gazebo transport.
//!
//! Sends 4-frame multipart messages matching Gazebo's wire
//! format:
//! ```text
//! Frame 0: Topic FQN (e.g., "@/partition@/world/...")
//! Frame 1: Sender address (e.g., "tcp://0.0.0.0:45678")
//! Frame 2: Protobuf-encoded message bytes
//! Frame 3: Message type name (e.g., "gz.msgs.Double")
//! ```

use std::marker::PhantomData;
use std::sync::mpsc;

use prost::Message;

use crate::error::{Error, Result};
use crate::partition::fully_qualified_name;

/// A Gazebo-compatible publisher that sends protobuf messages
/// via ZMQ PUB.
///
/// The ZMQ socket runs on a dedicated background thread (ZMQ is
/// not async). Messages are queued via an internal channel and
/// sent in order.
pub struct Publisher<T> {
    /// Channel to send publish requests to the background thread.
    tx: mpsc::SyncSender<PublishRequest>,
    /// Bound address of the ZMQ PUB socket.
    address: String,
    /// Topic name (not FQN, just the short name).
    topic: String,
    /// Protobuf message type name.
    msg_type: String,
    _handle: std::thread::JoinHandle<()>,
    _phantom: PhantomData<T>,
}

struct PublishRequest {
    fqn: String,
    data: Vec<u8>,
}

/// Channel depth for the publish queue.
const PUBLISH_CHANNEL_CAPACITY: usize = 256;

impl<T: Message + Default + Send + 'static> Publisher<T> {
    /// Create a publisher bound to an ephemeral TCP port.
    ///
    /// The `topic` is the short topic name (e.g.,
    /// "/model/ur10/joint/shoulder/cmd_pos"). The `msg_type` is
    /// the protobuf type (e.g., "gz.msgs.Double").
    pub fn bind(topic: &str, msg_type: &str) -> Result<Self> {
        let ctx = zmq::Context::new();
        let socket = ctx.socket(zmq::PUB).map_err(Error::Zmq)?;
        socket.set_linger(0).map_err(Error::Zmq)?;
        // Bind to ephemeral port -- OS assigns a free port.
        socket.bind("tcp://0.0.0.0:0").map_err(Error::Zmq)?;

        // Retrieve the actual bound address.
        let address = socket
            .get_last_endpoint()
            .map_err(Error::Zmq)?
            .map_err(|_| Error::BindFailed("failed to get last endpoint".into()))?;

        let (tx, rx) = mpsc::sync_channel::<PublishRequest>(PUBLISH_CHANNEL_CAPACITY);
        let address_clone = address.clone();
        let msg_type_clone = msg_type.to_string();

        let handle = std::thread::spawn(move || {
            publisher_thread(socket, rx, &address_clone, &msg_type_clone);
        });

        Ok(Self {
            tx,
            address,
            topic: topic.to_string(),
            msg_type: msg_type.to_string(),
            _handle: handle,
            _phantom: PhantomData,
        })
    }

    /// The TCP address the PUB socket is bound to.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The short topic name.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// The protobuf message type name.
    pub fn msg_type(&self) -> &str {
        &self.msg_type
    }

    /// Publish a message on the given partition.
    ///
    /// Builds the FQN from partition + topic, encodes the
    /// message as protobuf, and sends via the background ZMQ
    /// thread.
    pub fn publish(&self, partition: &str, msg: &T) -> Result<()> {
        let fqn = fully_qualified_name(partition, "", &self.topic);
        let data = msg.encode_to_vec();
        self.tx
            .send(PublishRequest { fqn, data })
            .map_err(|_| Error::ChannelClosed)
    }
}

/// Background thread that owns the ZMQ PUB socket and sends
/// messages.
fn publisher_thread(
    socket: zmq::Socket,
    rx: mpsc::Receiver<PublishRequest>,
    address: &str,
    msg_type: &str,
) {
    while let Ok(req) = rx.recv() {
        // 4-frame multipart matching Gazebo wire format.
        let frames: &[&[u8]] = &[
            req.fqn.as_bytes(),  // Frame 0: Topic FQN
            address.as_bytes(),  // Frame 1: Sender address
            &req.data,           // Frame 2: Protobuf data
            msg_type.as_bytes(), // Frame 3: Msg type name
        ];
        if let Err(e) = socket.send_multipart(frames, 0) {
            tracing::warn!("publisher send failed: {e}");
        }
    }
    // Channel closed -- publisher dropped, thread exits.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publisher_binds_and_reports_address() {
        let publisher =
            Publisher::<prost_types::Any>::bind("test_topic", "gz.msgs.Any").expect("should bind");
        let addr = publisher.address();
        assert!(
            addr.starts_with("tcp://"),
            "address should be tcp://, got {addr}"
        );
        assert!(!addr.contains(":0"), "should have resolved ephemeral port");
    }

    #[test]
    fn publishes_4_frame_multipart() {
        let ctx = zmq::Context::new();

        // Set up a raw SUB socket to verify frame format.
        let sub = ctx.socket(zmq::SUB).unwrap();
        sub.set_rcvtimeo(3000).unwrap();
        sub.set_subscribe(b"").unwrap(); // subscribe to all

        let publisher =
            Publisher::<prost_types::Any>::bind("/test/topic", "gz.msgs.Any").expect("should bind");
        sub.connect(publisher.address()).unwrap();

        // ZMQ slow joiner -- wait for subscription
        // propagation.
        std::thread::sleep(std::time::Duration::from_millis(300));

        let msg = prost_types::Any {
            type_url: "test".into(),
            value: vec![1, 2, 3],
        };
        publisher.publish("testhost:testuser", &msg).unwrap();

        let frames = sub.recv_multipart(0).expect("should receive message");
        assert_eq!(frames.len(), 4, "expected 4 frames");

        let fqn = String::from_utf8_lossy(&frames[0]);
        assert!(
            fqn.contains("testhost:testuser"),
            "FQN should contain partition: {fqn}"
        );
        assert!(
            fqn.contains("/test/topic"),
            "FQN should contain topic: {fqn}"
        );

        let sender = String::from_utf8_lossy(&frames[1]);
        assert!(
            sender.starts_with("tcp://"),
            "sender should be tcp address: {sender}"
        );

        // Frame 2 is protobuf data -- verify it decodes.
        let decoded = prost_types::Any::decode(frames[2].as_slice()).unwrap();
        assert_eq!(decoded.type_url, "test");
        assert_eq!(decoded.value, vec![1, 2, 3]);

        let msg_type = String::from_utf8_lossy(&frames[3]);
        assert_eq!(msg_type, "gz.msgs.Any");
    }
}
