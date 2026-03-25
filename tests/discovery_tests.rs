//! Tests for gz-transport discovery protocol
//!
//! These tests verify the correct format of discovery messages,
//! especially NEW_CONNECTION which is critical for receiving data from Gazebo.

use gz_transport::msgs::{Discovery, discovery};
use prost::Message;

/// Wire protocol version (must match Gazebo)
const WIRE_VERSION: u32 = 10;

/// Encode a discovery message with length prefix (gz-transport wire format)
fn encode_frame(msg: &Discovery) -> Vec<u8> {
    let payload = msg.encode_to_vec();
    let len = payload.len() as u16;
    let mut frame = Vec::with_capacity(2 + payload.len());
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(&payload);
    frame
}

/// Decode a discovery message from wire format
fn decode_frame(data: &[u8]) -> Option<Discovery> {
    if data.len() < 2 {
        return None;
    }
    let len = u16::from_le_bytes([data[0], data[1]]) as usize;
    if data.len() < 2 + len {
        return None;
    }
    Discovery::decode(&data[2..2 + len]).ok()
}

#[test]
fn test_subscribe_message_format() {
    // SUBSCRIBE uses Subscriber content with just the topic
    let topic = "@/test_partition@/world/default/clock";
    let process_uuid = "test-process-uuid-1234";

    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: process_uuid.to_string(),
        r#type: discovery::Type::Subscribe as i32,
        flags: None,
        disc_contents: Some(discovery::DiscContents::Sub(discovery::Subscriber {
            topic: topic.to_string(),
        })),
    };

    // Verify encoding/decoding roundtrip
    let frame = encode_frame(&msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    assert_eq!(decoded.version, WIRE_VERSION);
    assert_eq!(decoded.process_uuid, process_uuid);
    assert_eq!(decoded.r#type, discovery::Type::Subscribe as i32);

    match decoded.disc_contents {
        Some(discovery::DiscContents::Sub(sub)) => {
            assert_eq!(sub.topic, topic);
        }
        _ => panic!("Expected Subscriber content"),
    }
}

#[test]
fn test_new_connection_message_format() {
    // NEW_CONNECTION uses Publisher content with ctrl set to publisher's pUUID
    // This is how the publisher knows the message is addressed to them
    //
    // CRITICAL: msg_type must be "google.protobuf.Message" to act as wildcard.
    // The publisher's HasTopic() checks: _pub.MsgTypeName() == _type || _pub.MsgTypeName() == kGenericMessageType
    // where kGenericMessageType = "google.protobuf.Message"
    // Empty string would cause HasConnections() to return false for topics like pose/info
    // that only publish when subscribers are connected (unlike clock which always publishes).
    let topic = "@/test_partition@/world/default/clock";
    let our_process_uuid = "subscriber-process-uuid";
    let our_node_uuid = "subscriber-node-uuid";
    let publisher_puuid = "publisher-process-uuid"; // This goes in ctrl!
    let generic_msg_type = "google.protobuf.Message"; // Wildcard type

    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: our_process_uuid.to_string(),
        r#type: discovery::Type::NewConnection as i32,
        flags: None,
        disc_contents: Some(discovery::DiscContents::Pub(discovery::Publisher {
            topic: topic.to_string(),
            address: String::new(), // Subscriber doesn't provide address
            process_uuid: our_process_uuid.to_string(),
            node_uuid: our_node_uuid.to_string(),
            scope: discovery::publisher::Scope::All as i32,
            pub_type: Some(discovery::publisher::PubType::MsgPub(
                discovery::MessagePublisher {
                    ctrl: publisher_puuid.to_string(),      // KEY: Publisher's pUUID
                    msg_type: generic_msg_type.to_string(), // Wildcard for any topic
                    throttled: false,
                    msgs_per_sec: 0,
                },
            )),
        })),
    };

    // Verify encoding/decoding roundtrip
    let frame = encode_frame(&msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    assert_eq!(decoded.version, WIRE_VERSION);
    assert_eq!(decoded.process_uuid, our_process_uuid);
    assert_eq!(decoded.r#type, discovery::Type::NewConnection as i32);

    match decoded.disc_contents {
        Some(discovery::DiscContents::Pub(pub_info)) => {
            assert_eq!(pub_info.topic, topic);
            assert_eq!(pub_info.process_uuid, our_process_uuid);
            assert_eq!(pub_info.node_uuid, our_node_uuid);
            assert_eq!(pub_info.scope, discovery::publisher::Scope::All as i32);

            match pub_info.pub_type {
                Some(discovery::publisher::PubType::MsgPub(msg_pub)) => {
                    // The ctrl field must contain the publisher's pUUID
                    // This is how OnNewRegistration knows the message is for them:
                    // if (_pub.Ctrl() != this->pUuid) return;
                    assert_eq!(msg_pub.ctrl, publisher_puuid);
                    // msg_type must be the wildcard to match any topic type
                    assert_eq!(msg_pub.msg_type, generic_msg_type);
                }
                _ => panic!("Expected MsgPub"),
            }
        }
        _ => panic!("Expected Publisher content for NEW_CONNECTION"),
    }
}

#[test]
fn test_heartbeat_message_format() {
    let process_uuid = "heartbeat-process-uuid";

    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: process_uuid.to_string(),
        r#type: discovery::Type::Heartbeat as i32,
        flags: None,
        disc_contents: None,
    };

    let frame = encode_frame(&msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    assert_eq!(decoded.version, WIRE_VERSION);
    assert_eq!(decoded.r#type, discovery::Type::Heartbeat as i32);
    assert!(decoded.disc_contents.is_none());
}

#[test]
fn test_advertise_message_format() {
    // ADVERTISE is what publishers send to announce their topics
    let topic = "@/test_partition@/world/default/pose/info";
    let address = "tcp://192.168.1.100:12345";
    let ctrl_address = "tcp://192.168.1.100:12346";
    let process_uuid = "publisher-process-uuid";
    let node_uuid = "publisher-node-uuid";
    let msg_type = "gz.msgs.Pose_V";

    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: process_uuid.to_string(),
        r#type: discovery::Type::Advertise as i32,
        flags: None,
        disc_contents: Some(discovery::DiscContents::Pub(discovery::Publisher {
            topic: topic.to_string(),
            address: address.to_string(),
            process_uuid: process_uuid.to_string(),
            node_uuid: node_uuid.to_string(),
            scope: discovery::publisher::Scope::All as i32,
            pub_type: Some(discovery::publisher::PubType::MsgPub(
                discovery::MessagePublisher {
                    ctrl: ctrl_address.to_string(),
                    msg_type: msg_type.to_string(),
                    throttled: false,
                    msgs_per_sec: 0,
                },
            )),
        })),
    };

    let frame = encode_frame(&msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    assert_eq!(decoded.r#type, discovery::Type::Advertise as i32);

    match decoded.disc_contents {
        Some(discovery::DiscContents::Pub(pub_info)) => {
            assert_eq!(pub_info.topic, topic);
            assert_eq!(pub_info.address, address);

            match pub_info.pub_type {
                Some(discovery::publisher::PubType::MsgPub(msg_pub)) => {
                    assert_eq!(msg_pub.ctrl, ctrl_address);
                    assert_eq!(msg_pub.msg_type, msg_type);
                }
                _ => panic!("Expected MsgPub"),
            }
        }
        _ => panic!("Expected Publisher content"),
    }
}

#[test]
fn test_fully_qualified_name_format() {
    // Verify FQN format: @/partition@/topic
    let partition = "test_partition";
    let topic = "/world/default/clock";

    let fqn = format!("@/{}@{}", partition, topic);
    assert_eq!(fqn, "@/test_partition@/world/default/clock");

    // FQN must start with @/
    assert!(fqn.starts_with("@/"));
    // Must contain @/ separator before topic
    assert!(fqn.contains("@/world"));
}

#[test]
fn test_wire_version() {
    // gz-transport uses version 10 for the current protocol
    assert_eq!(WIRE_VERSION, 10);
}

#[test]
fn test_message_type_values() {
    // Verify message type enum values match Gazebo's expectations
    assert_eq!(discovery::Type::Uninitialized as i32, 0);
    assert_eq!(discovery::Type::Advertise as i32, 1);
    assert_eq!(discovery::Type::Subscribe as i32, 2);
    assert_eq!(discovery::Type::Unadvertise as i32, 3);
    assert_eq!(discovery::Type::Heartbeat as i32, 4);
    assert_eq!(discovery::Type::Bye as i32, 5);
    assert_eq!(discovery::Type::NewConnection as i32, 6);
    assert_eq!(discovery::Type::EndConnection as i32, 7);
}

// ============================================================================
// SERVICE DISCOVERY TESTS
// ============================================================================
//
// CRITICAL: Services use a DIFFERENT UDP port than topics!
// - Topics: port 10317 (GZ_DISCOVERY_MSG_PORT)
// - Services: port 10318 (GZ_DISCOVERY_SRV_PORT)
//
// Both use the same multicast address (239.255.0.7) and same Discovery message
// format, but services use ServicePublisher (SrvPub) instead of MessagePublisher.

/// Default discovery ports - services use a separate port from topics
#[test]
fn test_discovery_ports_are_separate() {
    // This is critical: gz-transport uses TWO separate UDP multicast ports
    // If we only listen on one, we miss half the discovery messages
    use gz_transport::config::{DEFAULT_MSG_DISCOVERY_PORT, DEFAULT_SRV_DISCOVERY_PORT};

    assert_eq!(DEFAULT_MSG_DISCOVERY_PORT, 10317, "Topic discovery port");
    assert_eq!(DEFAULT_SRV_DISCOVERY_PORT, 10318, "Service discovery port");
    assert_ne!(
        DEFAULT_MSG_DISCOVERY_PORT, DEFAULT_SRV_DISCOVERY_PORT,
        "Topics and services MUST use different ports"
    );
}

/// Service ADVERTISE uses ServicePublisher (SrvPub), not MessagePublisher (MsgPub)
#[test]
fn test_service_advertise_message_format() {
    // Services are advertised with ServicePublisher which has different fields:
    // - socket_id: ZMQ socket identity for routing
    // - request_type: protobuf type name for request (e.g., "gz.msgs.EntityFactory")
    // - response_type: protobuf type name for response (e.g., "gz.msgs.Boolean")
    let service = "@/test_partition@/world/default/create";
    let address = "tcp://192.168.1.100:12345";
    let process_uuid = "service-process-uuid";
    let node_uuid = "service-node-uuid";
    let socket_id = "service-socket-id-abc123";
    let request_type = "gz.msgs.EntityFactory";
    let response_type = "gz.msgs.Boolean";

    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: process_uuid.to_string(),
        r#type: discovery::Type::Advertise as i32,
        flags: None,
        disc_contents: Some(discovery::DiscContents::Pub(discovery::Publisher {
            topic: service.to_string(), // Services use "topic" field for service name
            address: address.to_string(),
            process_uuid: process_uuid.to_string(),
            node_uuid: node_uuid.to_string(),
            scope: discovery::publisher::Scope::All as i32,
            // KEY: Services use SrvPub, not MsgPub!
            pub_type: Some(discovery::publisher::PubType::SrvPub(
                discovery::ServicePublisher {
                    socket_id: socket_id.to_string(),
                    request_type: request_type.to_string(),
                    response_type: response_type.to_string(),
                },
            )),
        })),
    };

    let frame = encode_frame(&msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    assert_eq!(decoded.r#type, discovery::Type::Advertise as i32);

    match decoded.disc_contents {
        Some(discovery::DiscContents::Pub(pub_info)) => {
            assert_eq!(pub_info.topic, service);
            assert_eq!(pub_info.address, address);
            assert_eq!(pub_info.process_uuid, process_uuid);
            assert_eq!(pub_info.node_uuid, node_uuid);

            // Verify it's a ServicePublisher, not MessagePublisher
            match pub_info.pub_type {
                Some(discovery::publisher::PubType::SrvPub(srv_pub)) => {
                    assert_eq!(srv_pub.socket_id, socket_id);
                    assert_eq!(srv_pub.request_type, request_type);
                    assert_eq!(srv_pub.response_type, response_type);
                }
                Some(discovery::publisher::PubType::MsgPub(_)) => {
                    panic!("Service ADVERTISE must use SrvPub, not MsgPub!");
                }
                None => panic!("Expected pub_type"),
            }
        }
        _ => panic!("Expected Publisher content"),
    }
}

/// Verify EntityFactory service format (the specific service that was failing)
#[test]
fn test_entity_factory_service_format() {
    // This is the exact service that failed with "Service not found: /world/default/create"
    // It's advertised on port 10318, not 10317
    let service = "@/test_partition@/world/default/create";
    let address = "tcp://192.168.1.100:54321";
    let process_uuid = "gazebo-process-uuid";
    let node_uuid = "user-commands-node-uuid";
    let socket_id = "entity-factory-socket";

    let msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: process_uuid.to_string(),
        r#type: discovery::Type::Advertise as i32,
        flags: None,
        disc_contents: Some(discovery::DiscContents::Pub(discovery::Publisher {
            topic: service.to_string(),
            address: address.to_string(),
            process_uuid: process_uuid.to_string(),
            node_uuid: node_uuid.to_string(),
            scope: discovery::publisher::Scope::All as i32,
            pub_type: Some(discovery::publisher::PubType::SrvPub(
                discovery::ServicePublisher {
                    socket_id: socket_id.to_string(),
                    request_type: "gz.msgs.EntityFactory".to_string(),
                    response_type: "gz.msgs.Boolean".to_string(),
                },
            )),
        })),
    };

    let frame = encode_frame(&msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    // Verify this is a valid service advertisement
    match &decoded.disc_contents {
        Some(discovery::DiscContents::Pub(pub_info)) => match &pub_info.pub_type {
            Some(discovery::publisher::PubType::SrvPub(srv)) => {
                assert_eq!(srv.request_type, "gz.msgs.EntityFactory");
                assert_eq!(srv.response_type, "gz.msgs.Boolean");
            }
            _ => panic!("EntityFactory must be advertised as SrvPub"),
        },
        _ => panic!("Expected Publisher content"),
    }
}

// ============================================================================
// TOPIC DISCOVERY TESTS (continued)
// ============================================================================

/// Test that NEW_CONNECTION with wrong format (Subscriber instead of Publisher) would fail
/// This is the bug we fixed - sending Subscriber content instead of Publisher
#[test]
fn test_new_connection_wrong_format_detected() {
    // This is the WRONG format - using Subscriber for NEW_CONNECTION
    let topic = "@/test_partition@/world/default/clock";
    let process_uuid = "test-process-uuid";

    let wrong_msg = Discovery {
        header: None,
        version: WIRE_VERSION,
        process_uuid: process_uuid.to_string(),
        r#type: discovery::Type::NewConnection as i32,
        flags: None,
        // WRONG: Using Subscriber instead of Publisher
        disc_contents: Some(discovery::DiscContents::Sub(discovery::Subscriber {
            topic: topic.to_string(),
        })),
    };

    let frame = encode_frame(&wrong_msg);
    let decoded = decode_frame(&frame).expect("Failed to decode");

    // The message decodes, but it has the wrong content type
    match decoded.disc_contents {
        Some(discovery::DiscContents::Sub(_)) => {
            // This is the bug! NEW_CONNECTION should have Publisher content
            // gz-transport's OnNewRegistration expects Publisher and checks _pub.Ctrl()
        }
        Some(discovery::DiscContents::Pub(_)) => {
            panic!("This test verifies the wrong format - should be Sub not Pub");
        }
        None => panic!("Expected content"),
    }
}
