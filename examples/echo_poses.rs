//! Example: Echo poses from Gazebo
//!
//! Run with: cargo run --example echo_poses
//!
//! Requires a running Gazebo simulation, e.g.:
//!   gz sim shapes.sdf

use gz_transport::msgs::PoseV;
use gz_transport::{Config, Node, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    println!("Starting gz-transport node...");
    println!("Make sure Gazebo is running (e.g., `gz sim shapes.sdf`)");
    println!();

    // Create a node using config from environment (reads GZ_PARTITION, etc.)
    let config = Config::from_env();
    println!("Using partition: {}", config.effective_partition());
    let mut node = Node::with_config(config).await?;

    // Subscribe to the default world's pose topic
    let world_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "default".to_string());
    let pose_topic = format!("/world/{}/pose/info", world_name);

    println!("Subscribing to: {}", pose_topic);
    let mut subscriber = node.subscribe::<PoseV>(&pose_topic).await?;
    println!("Subscribed! Waiting for messages...\n");

    // Receive and print poses
    let mut count = 0;
    while let Some((poses, meta)) = subscriber.recv().await {
        count += 1;

        println!("=== Frame {} ({} poses) ===", count, poses.pose.len());
        println!("Topic: {}", meta.topic);
        println!("Type:  {}", meta.msg_type);

        for pose in &poses.pose {
            let pos = pose
                .position
                .as_ref()
                .map(|p| format!("({:.2}, {:.2}, {:.2})", p.x, p.y, p.z))
                .unwrap_or_else(|| "N/A".to_string());
            let ori = pose
                .orientation
                .as_ref()
                .map(|q| format!("({:.2}, {:.2}, {:.2}, {:.2})", q.w, q.x, q.y, q.z))
                .unwrap_or_else(|| "N/A".to_string());

            println!("  {} => pos: {}, ori: {}", pose.name, pos, ori);
        }
        println!();

        // Stop after 10 frames for demo purposes
        if count >= 10 {
            println!("Received 10 frames, exiting.");
            break;
        }
    }

    Ok(())
}
