//! Topic and partition naming utilities.

use crate::error::{Error, Result};

/// Invalid characters in topic/partition names.
#[allow(dead_code)] // Part of validation API
const INVALID_CHARS: &[char] = &['~', '@', ':'];

/// Validate a topic name.
#[allow(dead_code)] // Part of validation API
pub fn validate_topic(topic: &str) -> Result<()> {
    if topic.is_empty() {
        return Err(Error::InvalidTopicName {
            reason: "Topic name cannot be empty".to_string(),
        });
    }

    if topic.contains("//") {
        return Err(Error::InvalidTopicName {
            reason: "Topic name cannot contain '//'".to_string(),
        });
    }

    for c in INVALID_CHARS {
        if topic.contains(*c) {
            return Err(Error::InvalidTopicName {
                reason: format!("Topic name cannot contain '{}'", c),
            });
        }
    }

    if topic.chars().any(|c| c.is_whitespace()) {
        return Err(Error::InvalidTopicName {
            reason: "Topic name cannot contain whitespace".to_string(),
        });
    }

    Ok(())
}

/// Validate a partition name.
#[allow(dead_code)] // Part of validation API
pub fn validate_partition(partition: &str) -> Result<()> {
    if partition.is_empty() {
        return Err(Error::InvalidPartition {
            reason: "Partition name cannot be empty".to_string(),
        });
    }

    if partition.contains('@') {
        return Err(Error::InvalidPartition {
            reason: "Partition name cannot contain '@'".to_string(),
        });
    }

    Ok(())
}

/// Build a fully qualified topic name.
///
/// Format: `@<partition>@<namespace>/<topic>`
///
/// If topic starts with `/`, it's treated as absolute and namespace is skipped.
pub fn fully_qualified_name(partition: &str, namespace: &str, topic: &str) -> String {
    let ns = if namespace.is_empty() {
        "/".to_string()
    } else if namespace.starts_with('/') {
        namespace.to_string()
    } else {
        format!("/{}", namespace)
    };

    let ns = if ns.ends_with('/') {
        ns
    } else {
        format!("{}/", ns)
    };

    // If topic starts with /, it's absolute - skip namespace
    let full_topic = if topic.starts_with('/') {
        topic.to_string()
    } else {
        format!("{}{}", ns, topic)
    };

    // Gazebo uses @/partition@ format (with leading slash before partition)
    format!("@/{}@{}", partition, full_topic)
}

/// Parse a fully qualified topic name.
///
/// Returns (partition, topic) tuple.
/// Handles both `@partition@topic` and `@/partition@topic` formats
/// (Gazebo uses the latter with a leading slash).
pub fn parse_fully_qualified(fqn: &str) -> Option<(String, String)> {
    if !fqn.starts_with('@') {
        return None;
    }

    let rest = &fqn[1..];
    let second_at = rest.find('@')?;

    // Strip leading slash if present (Gazebo uses @/partition@topic format)
    let partition = rest[..second_at].trim_start_matches('/').to_string();
    let topic = rest[second_at + 1..].to_string();

    // Validate both parts are non-empty
    if partition.trim().is_empty() || topic.trim().is_empty() {
        return None;
    }

    Some((partition, topic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_topic() {
        assert!(validate_topic("/world/default/pose/info").is_ok());
        assert!(validate_topic("pose").is_ok());
        assert!(validate_topic("").is_err());
        assert!(validate_topic("/invalid//topic").is_err());
        assert!(validate_topic("invalid@topic").is_err());
        assert!(validate_topic("invalid topic").is_err());
    }

    #[test]
    fn test_fully_qualified_name() {
        // Gazebo uses @/partition@ format (with leading slash before partition)
        assert_eq!(
            fully_qualified_name("mypartition", "", "/world/default/pose"),
            "@/mypartition@/world/default/pose"
        );

        assert_eq!(
            fully_qualified_name("mypartition", "ns", "topic"),
            "@/mypartition@/ns/topic"
        );
    }

    #[test]
    fn test_parse_fully_qualified() {
        let (partition, topic) = parse_fully_qualified("@mypartition@/world/default/pose").unwrap();
        assert_eq!(partition, "mypartition");
        assert_eq!(topic, "/world/default/pose");
    }
}
