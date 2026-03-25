//! Example: List discovered Gazebo topics
//!
//! Run with: cargo run --example list_topics
//!
//! Requires a running Gazebo simulation, e.g.:
//!   gz sim shapes.sdf

use gz_transport::{Node, Result};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    println!("Starting gz-transport node...");
    println!("Make sure Gazebo is running (e.g., `gz sim shapes.sdf`)");
    println!();

    // Create a node (auto-discovers via UDP multicast)
    let node = Node::new(None).await?;

    // Wait for discovery
    println!("Waiting for discovery (5 seconds)...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // List discovered topics
    let topics = node.topics();
    println!("\nDiscovered {} topics:", topics.len());
    for topic in &topics {
        println!("  Topic: {}", topic.topic);
        println!("    Type: {}", topic.msg_type);
        for pub_info in &topic.publishers {
            println!(
                "    Publisher: {} ({})",
                pub_info.address, pub_info.node_uuid
            );
        }
        println!();
    }

    // List discovered services
    let services = node.services();
    println!("Discovered {} services:", services.len());
    for service in &services {
        println!("  Service: {}", service.service);
        println!("    Request: {}", service.request_type);
        println!("    Response: {}", service.response_type);
        println!("    Address: {}", service.address);
        println!();
    }

    Ok(())
}
