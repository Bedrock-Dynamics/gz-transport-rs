# gz-transport

Pure Rust implementation of the Gazebo Transport protocol.

## Features

- **No Gazebo C++ stack required** - Only depends on libzmq (system or vendored via zmq-sys)
- **Version-agnostic** - Works with Gazebo Harmonic, Ionic, Jetty, and future versions
- **Async-first** - Built on tokio for efficient async I/O
- **Cross-platform** - Linux, macOS, Windows
- **Docker-compatible** - Works with containerized Gazebo instances

## Quick Start

```rust
use gz_transport::{Node, Result};
use gz_transport::msgs::PoseV;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a node (auto-discovers Gazebo via UDP multicast)
    let mut node = Node::new(None).await?;

    // Subscribe to entity poses
    let mut sub = node.subscribe::<PoseV>("/world/default/pose/info").await?;

    // Receive messages
    while let Some((poses, _meta)) = sub.recv().await {
        println!("Received {} poses", poses.pose.len());
    }

    Ok(())
}
```

## Protocol Support

| Feature | Status |
|---------|--------|
| UDP multicast discovery | ✅ |
| ZMQ PUB/SUB topics | ✅ |
| ZMQ DEALER/ROUTER services | ✅ |
| Protobuf messages | ✅ |
| Partition isolation | ✅ |

## Message Types

The crate includes commonly used Gazebo message types:

- `Pose`, `PoseV` - Entity poses
- `Vector3d`, `Quaternion` - Geometry primitives
- `WorldControl`, `WorldReset` - Simulation control
- `Boolean`, `Empty` - Service responses
- `Clock`, `Time` - Timestamps
- `Header` - Message metadata

## Examples

```bash
# List discovered topics and services
cargo run --example list_topics

# Echo poses from default world
cargo run --example echo_poses

# Pause/resume simulation
cargo run --example pause_world pause
cargo run --example pause_world resume
```

## Configuration

The crate respects standard Gazebo environment variables:

- `GZ_PARTITION` - Partition name for topic/service isolation
- Default partition: `<hostname>:<username>`

## Docker Support

For containerized Gazebo instances, use manual endpoint configuration:

```rust
use gz_transport::{Node, Config};

let config = Config::builder()
    .partition(Some("docker"))
    .build();

let node = Node::with_config(config).await?;
```

## System Dependencies

This crate requires **libzmq** (ZeroMQ). The `zmq` crate can either link against
a system-installed libzmq or build it from source via `zmq-sys`/`zeromq-src`.

On Debian/Ubuntu: `sudo apt install libzmq3-dev`
On macOS: `brew install zeromq`

## Roadmap

- Expand proto coverage (all `gz.msgs.*` types)
- Service hosting (ROUTER replier sockets)
- `cargo publish` automation + CI

## License

MIT OR Apache-2.0
