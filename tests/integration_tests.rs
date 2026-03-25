//! Integration tests for gz-transport
//!
//! These tests require a running Gazebo Sim instance.
//! Run with: GZ_PARTITION=test_local cargo test --test integration_tests -- --ignored
//!
//! Before running, start Gazebo:
//!   GZ_PARTITION=test_local gz sim shapes.sdf

use gz_transport::msgs::{Clock, PoseV};
use gz_transport::{Config, Node};
use std::time::Duration;

/// Test subscribing to clock topic and receiving messages
///
/// This test verifies:
/// 1. Discovery finds the clock topic publisher
/// 2. ZMQ subscription is established
/// 3. NEW_CONNECTION is sent correctly (with Publisher content)
/// 4. Messages are actually received from Gazebo
#[tokio::test]
#[ignore] // Requires running Gazebo
async fn test_subscribe_clock_receives_messages() {
    // Use partition from environment or default
    let config = Config::from_env();
    println!("Using partition: {}", config.effective_partition());

    let mut node = Node::with_config(config)
        .await
        .expect("Failed to create node");

    // Subscribe to clock topic
    let topic = "/world/shapes/clock";
    println!("Subscribing to: {}", topic);

    let mut subscriber = node
        .subscribe::<Clock>(topic)
        .await
        .expect("Failed to subscribe to clock");

    println!("Subscribed! Waiting for messages...");

    // Wait for at least one message with timeout
    let timeout = Duration::from_secs(10);
    let result = tokio::time::timeout(timeout, subscriber.recv()).await;

    match result {
        Ok(Some((clock, meta))) => {
            println!("Received clock message!");
            println!("  Topic: {}", meta.topic);
            println!("  Type: {}", meta.msg_type);
            if let Some(sim) = clock.sim {
                println!("  Sim time: {}.{}", sim.sec, sim.nsec);
            }
            // Success!
        }
        Ok(None) => {
            panic!("Subscriber channel closed unexpectedly");
        }
        Err(_) => {
            panic!(
                "Timeout waiting for clock message after {:?}. \
                Make sure Gazebo is running with the correct partition.",
                timeout
            );
        }
    }
}

/// Test subscribing to pose topic and receiving entity poses
#[tokio::test]
#[ignore] // Requires running Gazebo
async fn test_subscribe_pose_receives_messages() {
    let config = Config::from_env();
    println!("Using partition: {}", config.effective_partition());

    let mut node = Node::with_config(config)
        .await
        .expect("Failed to create node");

    // Subscribe to pose topic
    let topic = "/world/shapes/pose/info";
    println!("Subscribing to: {}", topic);

    let mut subscriber = node
        .subscribe::<PoseV>(topic)
        .await
        .expect("Failed to subscribe to poses");

    println!("Subscribed! Waiting for messages...");

    // Wait for at least one message with timeout
    let timeout = Duration::from_secs(10);
    let result = tokio::time::timeout(timeout, subscriber.recv()).await;

    match result {
        Ok(Some((poses, meta))) => {
            println!("Received pose message with {} entities!", poses.pose.len());
            println!("  Topic: {}", meta.topic);
            println!("  Type: {}", meta.msg_type);

            for pose in &poses.pose {
                let pos = pose
                    .position
                    .as_ref()
                    .map(|p| format!("({:.2}, {:.2}, {:.2})", p.x, p.y, p.z))
                    .unwrap_or_else(|| "N/A".to_string());
                println!("  {} => {}", pose.name, pos);
            }

            assert!(!poses.pose.is_empty(), "Expected at least one entity pose");
        }
        Ok(None) => {
            panic!("Subscriber channel closed unexpectedly");
        }
        Err(_) => {
            panic!(
                "Timeout waiting for pose message after {:?}. \
                Make sure Gazebo is running with the correct partition.",
                timeout
            );
        }
    }
}

/// Test that multiple subscriptions work
#[tokio::test]
#[ignore] // Requires running Gazebo
async fn test_multiple_subscriptions() {
    let config = Config::from_env();

    let mut node = Node::with_config(config)
        .await
        .expect("Failed to create node");

    // Subscribe to both clock and pose
    let mut clock_sub = node
        .subscribe::<Clock>("/world/shapes/clock")
        .await
        .expect("Failed to subscribe to clock");

    let mut pose_sub = node
        .subscribe::<PoseV>("/world/shapes/pose/info")
        .await
        .expect("Failed to subscribe to poses");

    let timeout = Duration::from_secs(10);

    // Receive from both
    let clock_result = tokio::time::timeout(timeout, clock_sub.recv()).await;
    let pose_result = tokio::time::timeout(timeout, pose_sub.recv()).await;

    assert!(
        clock_result.is_ok() && clock_result.unwrap().is_some(),
        "Should receive clock"
    );
    assert!(
        pose_result.is_ok() && pose_result.unwrap().is_some(),
        "Should receive poses"
    );
}

/// Test calling a service (WorldControl)
#[tokio::test]
#[ignore] // Requires running Gazebo
async fn test_service_call_world_control() {
    use gz_transport::msgs::{Boolean, WorldControl};

    let config = Config::from_env();
    println!("Using partition: {}", config.effective_partition());

    let mut node = Node::with_config(config)
        .await
        .expect("Failed to create node");

    // Call world control service to pause simulation
    let service = "/world/shapes/control";
    println!("Calling service: {}", service);

    let request = WorldControl {
        header: None,
        pause: true,
        step: false,
        multi_step: 0,
        reset: None,
        seed: 0,
        run_to_sim_time: None,
    };

    let result = node
        .call_service::<WorldControl, Boolean>(service, request)
        .await;

    match result {
        Ok(response) => {
            println!("Service call succeeded! Response: {}", response.data);
        }
        Err(e) => {
            panic!("Service call failed: {:?}", e);
        }
    }
}

/// Test discovery timeout when topic doesn't exist
#[tokio::test]
#[ignore] // Requires running Gazebo
async fn test_discovery_timeout_nonexistent_topic() {
    let mut config = Config::from_env();
    config.discovery_timeout = Duration::from_secs(2); // Short timeout

    let mut node = Node::with_config(config)
        .await
        .expect("Failed to create node");

    // Try to subscribe to non-existent topic
    let result = node.subscribe::<Clock>("/nonexistent/topic").await;

    assert!(
        result.is_err(),
        "Should fail to subscribe to non-existent topic"
    );
    println!("Got expected error: {:?}", result.err());
}
