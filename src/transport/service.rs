//! ZeroMQ service client implementation matching gz-transport C++ exactly.
//!
//! Gazebo uses ROUTER sockets for all service communication:
//! - requester: ROUTER socket for sending requests (with routing_id set)
//! - responseReceiver: ROUTER socket for receiving responses (with routing_id set)
//! - replier: ROUTER socket for handling requests
//!
//! Key implementation details from C++ source:
//! 1. All ROUTER sockets have routing_id set (UUID string)
//! 2. 100ms sleep after connect to allow ZMQ to establish connection
//! 3. First frame of request is responder's socket_id for routing
//! 4. router_mandatory is set on sockets

use crate::error::{Error, Result};
use parking_lot::Mutex;
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use tracing::{debug, trace, warn};

/// A client for calling Gazebo services.
pub struct ServiceClient {
    /// Channel for sending requests to the worker thread
    request_tx: std::sync::mpsc::Sender<ServiceRequest>,
    /// Pending requests by UUID
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ServiceResponse>>>>,
    /// Our node UUID
    node_uuid: String,
    /// Our response receiver ID (routing_id)
    response_receiver_id: String,
    /// Service name (plain, without partition)
    #[allow(dead_code)] // Stored for debugging/logging purposes
    service: String,
    /// Fully qualified service name (with partition)
    service_fqn: String,
    /// The service's socket_id (routing destination)
    responder_socket_id: String,
    /// Shutdown signal for worker thread (follows Tokio runtime pattern)
    shutdown: Arc<AtomicBool>,
    /// Worker thread handle
    _handle: std::thread::JoinHandle<()>,
}

/// Internal request structure
struct ServiceRequest {
    frames: Vec<Vec<u8>>,
    /// Channel to report send errors immediately
    error_tx: oneshot::Sender<Option<String>>,
}

/// Response from a service call.
#[derive(Debug)]
struct ServiceResponse {
    /// Response data (protobuf bytes).
    data: Vec<u8>,
    /// Whether the call succeeded.
    success: bool,
}

impl ServiceClient {
    /// Connect to a service endpoint.
    ///
    /// # Arguments
    /// * `endpoint` - ZMQ TCP address (e.g., "tcp://127.0.0.1:12345")
    /// * `service` - Service name (plain, e.g., "/world/shapes/control")
    /// * `partition` - Partition name for building FQN
    /// * `socket_id` - The service's socket identity (used for routing requests)
    /// * `host_ip` - Optional host IP override (from Config.host_ip / GZ_IP)
    /// * `response_port` - Optional fixed port for response receiver (avoids VPN conflicts)
    pub async fn connect(
        endpoint: &str,
        service: &str,
        partition: &str,
        socket_id: &str,
        host_ip: Option<std::net::Ipv4Addr>,
        response_port: Option<u16>,
    ) -> Result<Self> {
        let node_uuid = uuid::Uuid::new_v4().to_string();
        let response_receiver_id = uuid::Uuid::new_v4().to_string();

        // Build fully qualified service name with partition
        // Gazebo registers services under FQN like @/partition@/world/shapes/control
        let service_fqn = crate::partition::fully_qualified_name(partition, "", service);

        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<ServiceResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Channel for sending requests to worker thread
        let (request_tx, request_rx) = std::sync::mpsc::channel::<ServiceRequest>();

        // Shutdown signal for graceful worker thread termination
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        // Clone for worker thread
        let worker_endpoint = endpoint.to_string();
        let worker_pending = pending.clone();
        let worker_response_id = response_receiver_id.clone();
        let worker_node_uuid = node_uuid.clone();

        // Spawn worker thread
        let handle = std::thread::spawn(move || {
            if let Err(e) = service_worker(
                &worker_endpoint,
                &worker_response_id,
                &worker_node_uuid,
                request_rx,
                worker_pending,
                host_ip,
                response_port,
                shutdown_clone,
            ) {
                warn!("Service worker error: {}", e);
            }
        });

        // Wait for connection to establish (matching C++ 100ms sleep)
        tokio::time::sleep(Duration::from_millis(100)).await;

        debug!(
            "ServiceClient connected to {} for service '{}'",
            endpoint, service
        );

        Ok(Self {
            request_tx,
            pending,
            node_uuid,
            response_receiver_id,
            service: service.to_string(),
            service_fqn,
            responder_socket_id: socket_id.to_string(),
            shutdown,
            _handle: handle,
        })
    }

    /// Call the service with a request.
    ///
    /// # Type Parameters
    /// * `Req` - Request message type
    /// * `Res` - Response message type
    pub async fn call<Req, Res>(&self, request: Req, timeout: Duration) -> Result<Res>
    where
        Req: Message,
        Res: Message + Default,
    {
        let request_uuid = uuid::Uuid::new_v4().to_string();

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Create error channel for immediate send failures
        let (error_tx, error_rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.lock();
            pending.insert(request_uuid.clone(), tx);
        }

        // Build request frames matching C++ gz-transport EXACTLY:
        // Frame 1: responserId (responder's socket_id for ROUTER routing)
        // Frame 2: topic (service name)
        // Frame 3: myRequesterAddress (where to send response)
        // Frame 4: myId (responseReceiverId - our routing_id)
        // Frame 5: nodeUuid
        // Frame 6: reqUuid
        // Frame 7: data (serialized protobuf)
        // Frame 8: reqType
        // Frame 9: repType

        let request_type = std::any::type_name::<Req>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown");
        let response_type = std::any::type_name::<Res>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown");

        let request_data = request.encode_to_vec();

        // Build 9 frames exactly as C++ does
        let frames = vec![
            self.responder_socket_id.as_bytes().to_vec(), // Frame 1: responserId
            self.service_fqn.as_bytes().to_vec(),         // Frame 2: topic (FQN with partition)
            Vec::new(), // Frame 3: myRequesterAddress (filled by worker)
            self.response_receiver_id.as_bytes().to_vec(), // Frame 4: myId
            self.node_uuid.as_bytes().to_vec(), // Frame 5: nodeUuid
            request_uuid.as_bytes().to_vec(), // Frame 6: reqUuid
            request_data, // Frame 7: data
            format!("gz.msgs.{}", request_type).into_bytes(), // Frame 8: reqType
            format!("gz.msgs.{}", response_type).into_bytes(), // Frame 9: repType
        ];

        // Send request to worker
        self.request_tx
            .send(ServiceRequest { frames, error_tx })
            .map_err(|_| Error::ChannelClosed)?;

        // Check for immediate send error
        match error_rx.await {
            Ok(Some(err)) => {
                // Clean up pending
                let mut pending = self.pending.lock();
                pending.remove(&request_uuid);
                return Err(Error::ServiceCallFailed(err));
            }
            Ok(None) => {
                // Send succeeded, continue waiting for response
            }
            Err(_) => {
                // Worker died
                let mut pending = self.pending.lock();
                pending.remove(&request_uuid);
                return Err(Error::ChannelClosed);
            }
        }

        trace!("Sent service request: {}", request_uuid);

        // Wait for response with timeout
        let result = tokio::time::timeout(timeout, rx).await;

        // Clean up pending
        {
            let mut pending = self.pending.lock();
            pending.remove(&request_uuid);
        }

        match result {
            Ok(Ok(response)) => {
                if response.success {
                    let decoded = Res::decode(response.data.as_slice())?;
                    Ok(decoded)
                } else {
                    Err(Error::ServiceCallFailed(
                        "Service returned failure".to_string(),
                    ))
                }
            }
            Ok(Err(_)) => Err(Error::ChannelClosed),
            Err(_) => Err(Error::ServiceTimeout(timeout)),
        }
    }
}

impl Drop for ServiceClient {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        debug!("ServiceClient dropped, worker shutdown initiated");
    }
}

/// Get the local IP address that can reach the given endpoint.
/// This is important for advertising the correct return address.
///
/// # Arguments
/// * `endpoint` - The service endpoint we're connecting to
/// * `host_ip` - Optional override from Config (sourced from GZ_IP)
fn get_local_ip_for_endpoint(endpoint: &str, host_ip: Option<std::net::Ipv4Addr>) -> String {
    // Extract remote IP from endpoint
    let remote_ip = endpoint
        .strip_prefix("tcp://")
        .and_then(|s| s.split(':').next())
        .unwrap_or("127.0.0.1");

    // Use explicit host_ip if provided (from Config/GZ_IP)
    // Always use the configured IP, even for local connections
    // Gazebo may expect the external IP for connection routing
    if let Some(ip) = host_ip {
        return ip.to_string();
    }

    // If remote is localhost, use localhost
    if remote_ip == "127.0.0.1" || remote_ip == "localhost" {
        return "127.0.0.1".to_string();
    }

    // Try to determine local IP by creating a UDP socket and connecting
    // This doesn't send any data, just lets the OS pick the right interface
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect(format!("{}:1", remote_ip)).is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                return local_addr.ip().to_string();
            }
        }
    }

    // Fallback to 0.0.0.0 (will likely fail for remote services)
    "0.0.0.0".to_string()
}

/// Worker thread for handling ZMQ socket operations
#[allow(clippy::too_many_arguments)]
fn service_worker(
    endpoint: &str,
    response_receiver_id: &str,
    node_uuid: &str,
    request_rx: std::sync::mpsc::Receiver<ServiceRequest>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ServiceResponse>>>>,
    host_ip: Option<std::net::Ipv4Addr>,
    response_port: Option<u16>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    // Create ZMQ context
    let ctx = zmq::Context::new();

    // === REQUESTER SOCKET (ROUTER) ===
    // Matches C++: this->dataPtr->requester = new zmq::socket_t(*context, ZMQ_ROUTER)
    let requester = ctx
        .socket(zmq::ROUTER)
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to create ROUTER socket: {}", e),
        })?;

    // Set routing_id on requester - ALL ROUTER sockets need an identity
    // Use node_uuid as the identity (matches C++ pattern)
    requester
        .set_identity(node_uuid.as_bytes())
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to set requester identity: {}", e),
        })?;

    // Set socket options matching C++
    requester
        .set_linger(0)
        .map_err(|e| warn!("set_linger on requester: {}", e))
        .ok();
    requester
        .set_router_mandatory(true)
        .map_err(|e| warn!("set_router_mandatory on requester: {}", e))
        .ok();

    // Set up monitor on requester to track connection status
    let req_monitor_addr = format!("inproc://monitor-requester-{}", node_uuid);
    requester
        .monitor(&req_monitor_addr, zmq::SocketEvent::ALL as i32)
        .ok();
    let req_monitor = ctx.socket(zmq::PAIR).ok();
    if let Some(ref mon) = req_monitor {
        mon.connect(&req_monitor_addr).ok();
        mon.set_rcvtimeo(0).ok();
    }

    // Connect to the service endpoint
    requester
        .connect(endpoint)
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to connect requester: {}", e),
        })?;

    // === RESPONSE RECEIVER SOCKET (ROUTER) ===
    // Matches C++: this->dataPtr->responseReceiver = new zmq::socket_t(*context, ZMQ_ROUTER)
    let response_receiver = ctx
        .socket(zmq::ROUTER)
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to create response receiver socket: {}", e),
        })?;

    // Set routing_id BEFORE bind - this is critical!
    // Matches C++: this->dataPtr->responseReceiver->set(zmq::sockopt::routing_id, id)
    response_receiver
        .set_identity(response_receiver_id.as_bytes())
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to set response receiver identity: {}", e),
        })?;

    response_receiver
        .set_rcvtimeo(100)
        .map_err(|e| warn!("set_rcvtimeo on response_receiver: {}", e))
        .ok();
    response_receiver
        .set_linger(0)
        .map_err(|e| warn!("set_linger on response_receiver: {}", e))
        .ok();
    response_receiver
        .set_router_mandatory(true)
        .map_err(|e| warn!("set_router_mandatory on response_receiver: {}", e))
        .ok();

    // Determine the correct local IP to advertise
    let local_ip = get_local_ip_for_endpoint(endpoint, host_ip);

    // Set up socket monitor BEFORE bind to catch LISTENING event
    let monitor_addr = format!("inproc://monitor-response-{}", response_receiver_id);
    response_receiver
        .monitor(&monitor_addr, zmq::SocketEvent::ALL as i32)
        .ok();
    let monitor_socket = ctx.socket(zmq::PAIR).ok();
    if let Some(ref mon) = monitor_socket {
        mon.connect(&monitor_addr).ok();
        mon.set_rcvtimeo(0).ok(); // Non-blocking
    }

    // Bind to specified port or ephemeral port on 0.0.0.0
    // Using a fixed port avoids conflicts with VPNs like Tailscale
    let bind_addr = match response_port {
        Some(port) => format!("tcp://0.0.0.0:{}", port),
        None => "tcp://0.0.0.0:*".to_string(),
    };
    response_receiver
        .bind(&bind_addr)
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to bind response receiver to {}: {}", bind_addr, e),
        })?;

    // Get our bound port and build advertised address
    let bound_endpoint = response_receiver
        .get_last_endpoint()
        .map_err(|e| Error::ConnectionFailed {
            address: endpoint.to_string(),
            message: format!("Failed to get bound address: {}", e),
        })?
        .unwrap_or_default();

    // Extract port and build advertised address with our reachable IP
    let port = bound_endpoint.rsplit(':').next().unwrap_or("0");
    let our_address = format!("tcp://{}:{}", local_ip, port);

    // CRITICAL: Wait for ZMQ connection to fully establish
    // C++ uses 100ms, but we may need more for ROUTER-ROUTER handshake
    std::thread::sleep(Duration::from_millis(500));

    debug!(
        "Service worker connected to {}, response receiver at {}",
        endpoint, our_address
    );

    // Track connection state
    let mut requester_connected = false;
    let mut pending_request: Option<ServiceRequest> = None;

    loop {
        // Check for shutdown request (graceful termination)
        if shutdown.load(Ordering::Relaxed) {
            debug!("Service worker shutdown requested");
            break;
        }

        // Helper closure to parse ZMQ monitor event type
        let parse_event = |event_data: &[u8]| -> u16 {
            if event_data.len() >= 2 {
                u16::from_le_bytes([event_data[0], event_data[1]])
            } else {
                0
            }
        };

        // Check requester socket monitor and track CONNECTED state
        if let Some(ref mon) = req_monitor {
            if let Ok(event_msg) = mon.recv_msg(zmq::DONTWAIT) {
                let event_type = parse_event(event_msg.as_ref());
                let _ = mon.recv_msg(zmq::DONTWAIT); // Drain address frame

                // Track connection state - wait for full handshake
                // HANDSHAKE_SUCCEEDED (4096) - connection fully ready
                if event_type == 4096 && !requester_connected {
                    requester_connected = true;
                    debug!("Service requester connection established");
                }
            }
        }

        // Check response receiver socket monitor
        if let Some(ref mon) = monitor_socket {
            if let Ok(_event_msg) = mon.recv_msg(zmq::DONTWAIT) {
                let _ = mon.recv_msg(zmq::DONTWAIT); // Drain address frame
            }
        }

        // Check for outgoing requests (non-blocking)
        // Queue requests until connection is established
        if pending_request.is_none() {
            if let Ok(request) = request_rx.try_recv() {
                pending_request = Some(request);
            }
        }

        // Only send if connected
        if let Some(mut request) = pending_request.take() {
            if !requester_connected {
                // Not connected yet, put request back
                pending_request = Some(request);
            } else {
                // Update Frame 3 (myRequesterAddress) with our advertised address
                // Frame indices: [0]=responserId, [1]=topic, [2]=return_addr, [3]=myId, ...
                if request.frames.len() >= 3 {
                    request.frames[2] = our_address.as_bytes().to_vec();
                }

                // Send multipart message atomically
                let send_result = requester.send_multipart(&request.frames, 0);
                let send_error = match send_result {
                    Ok(_) => None,
                    Err(e) => {
                        let err_msg = format!("Failed to send multipart: {}", e);
                        warn!("{}", err_msg);
                        Some(err_msg)
                    }
                };

                // Report send result back to caller
                let _ = request.error_tx.send(send_error.clone());

                if send_error.is_none() {
                    trace!("Service request sent successfully");
                }
            }
        }

        // Check for incoming responses on response_receiver (non-blocking)
        match response_receiver.recv_multipart(zmq::DONTWAIT) {
            Ok(frames) => {
                // Response format (ROUTER prepends sender identity):
                // Gazebo ROUTER sends: [dstId, topic, nodeUuid, reqUuid, data, result]
                // dstId is used for routing (our identity) and stripped
                // Our ROUTER prepends sender identity
                //
                // Frame 0: Sender routing ID (auto-prepended by ROUTER)
                // Frame 1: topic
                // Frame 2: nodeUuid
                // Frame 3: reqUuid
                // Frame 4: response data
                // Frame 5: result ("1" or "0")

                if frames.len() < 6 {
                    warn!(
                        "Invalid service response: expected at least 6 frames, got {}",
                        frames.len()
                    );
                    continue;
                }

                // Skip frame 0 (sender identity), parse from frame 1
                let request_uuid = String::from_utf8_lossy(&frames[3]).to_string();
                let data = frames[4].clone();
                let result = String::from_utf8_lossy(&frames[5]).to_string();

                let success = result == "1";

                trace!(
                    "Received service response: {} (success: {})",
                    request_uuid, success
                );

                // Find and complete pending request
                let sender = {
                    let mut pending = pending.lock();
                    pending.remove(&request_uuid)
                };

                if let Some(tx) = sender {
                    let _ = tx.send(ServiceResponse { data, success });
                } else {
                    warn!("Received response for unknown request: {}", request_uuid);
                }
            }
            Err(zmq::Error::EAGAIN) => {
                // No data available - continue loop
            }
            Err(e) => {
                warn!("Service client recv error: {}", e);
                break;
            }
        }

        // Small sleep to avoid busy loop
        std::thread::sleep(Duration::from_millis(1));
    }

    debug!("Service worker stopped");
    Ok(())
}
