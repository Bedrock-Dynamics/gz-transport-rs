//! Example: Pause/resume Gazebo world via service call
//!
//! Run with: cargo run --example pause_world [pause|resume|step|reset]
//!
//! Requires a running Gazebo simulation, e.g.:
//!   gz sim shapes.sdf

use gz_transport::msgs::{Boolean, WorldControl, WorldReset};
use gz_transport::{Node, Result};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    // Parse command
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "pause".to_string());

    println!("Starting gz-transport node...");
    println!("Make sure Gazebo is running (e.g., `gz sim shapes.sdf`)");
    println!();

    // Create a node
    let mut node = Node::new(None).await?;

    // Build the control request
    let world_name = "default";
    let service = format!("/world/{}/control", world_name);

    let request = match command.as_str() {
        "pause" => {
            println!("Pausing simulation...");
            WorldControl {
                pause: true,
                ..Default::default()
            }
        }
        "resume" => {
            println!("Resuming simulation...");
            WorldControl {
                pause: false,
                ..Default::default()
            }
        }
        "step" => {
            println!("Stepping simulation (1 frame)...");
            WorldControl {
                pause: true,
                multi_step: 1,
                ..Default::default()
            }
        }
        "reset" => {
            println!("Resetting simulation...");
            WorldControl {
                reset: Some(WorldReset {
                    all: true,
                    ..Default::default()
                }),
                ..Default::default()
            }
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            eprintln!("Usage: pause_world [pause|resume|step|reset]");
            return Ok(());
        }
    };

    // Call the service
    println!("Calling service: {}", service);
    let response: Boolean = node
        .call_service_with_timeout(&service, request, Duration::from_secs(5))
        .await?;

    println!("Response: data={}", response.data);
    println!("Done!");

    Ok(())
}
