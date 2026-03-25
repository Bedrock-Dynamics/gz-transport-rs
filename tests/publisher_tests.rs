//! Publisher integration tests.
//!
//! Verify pub/sub roundtrip within a single process using the discovery
//! protocol (no running Gazebo required — uses multicast loopback).

/// Two Node instances: one publishes, one subscribes.
/// Verifies the full discovery → ZMQ PUB/SUB roundtrip.
#[tokio::test]
async fn pubsub_roundtrip_same_process() {
    let partition = "roundtrip_test";

    // Publisher node.
    let mut pub_node = gz_transport::Node::new(Some(partition)).await.unwrap();
    let publisher = pub_node
        .advertise::<gz_transport::msgs::Clock>("/test/roundtrip", "gz.msgs.Clock")
        .await
        .unwrap();

    // Wait for ADVERTISE to propagate via multicast loopback.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Subscriber node.
    let mut sub_node = gz_transport::Node::new(Some(partition)).await.unwrap();
    let mut subscriber = sub_node
        .subscribe::<gz_transport::msgs::Clock>("/test/roundtrip")
        .await
        .unwrap();

    // Wait for ZMQ SUB subscription propagation.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Publish a Clock message.
    let clock = gz_transport::msgs::Clock {
        header: None,
        sim: Some(gz_transport::msgs::Time { sec: 42, nsec: 123 }),
        real: None,
        system: None,
    };
    publisher.publish(partition, &clock).unwrap();

    // Receive it.
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(3), subscriber.recv()).await;

    match timeout {
        Ok(Some((received, meta))) => {
            let sim = received.sim.expect("sim time should be set");
            assert_eq!(sim.sec, 42);
            assert_eq!(sim.nsec, 123);
            assert!(
                meta.topic.contains("/test/roundtrip"),
                "meta topic: {}",
                meta.topic
            );
        }
        Ok(None) => panic!("subscriber channel closed unexpectedly"),
        Err(_) => panic!("timed out waiting for published message"),
    }
}

/// Publishing without subscribers should not error.
#[tokio::test]
async fn publish_without_subscribers_is_ok() {
    let mut node = gz_transport::Node::new(Some("no_sub_test")).await.unwrap();
    let publisher = node
        .advertise::<gz_transport::msgs::Clock>("/test/no_sub", "gz.msgs.Clock")
        .await
        .unwrap();

    let clock = gz_transport::msgs::Clock {
        header: None,
        sim: Some(gz_transport::msgs::Time { sec: 1, nsec: 0 }),
        real: None,
        system: None,
    };

    assert!(publisher.publish("no_sub_test", &clock).is_ok());
}

/// Node::advertise creates a publisher with correct metadata.
#[tokio::test]
async fn node_advertise_creates_publisher() {
    let mut node = gz_transport::Node::new(Some("adv_test")).await.unwrap();
    let publisher = node
        .advertise::<prost_types::Any>("/test/node_pub", "gz.msgs.Any")
        .await
        .unwrap();

    assert!(!publisher.address().is_empty());
    assert_eq!(publisher.topic(), "/test/node_pub");
    assert_eq!(publisher.msg_type(), "gz.msgs.Any");
}
