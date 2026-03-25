//! Discovery message framing.

use crate::error::{Error, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use prost::Message;

/// Maximum message size for discovery frames (u16::MAX = 65535 bytes).
const MAX_FRAME_SIZE: usize = u16::MAX as usize;

/// Encode a discovery frame.
///
/// Format: [2-byte length (LE)][protobuf body]
///
/// Returns Error if message exceeds MAX_FRAME_SIZE (65535 bytes).
pub fn encode_frame<M: Message>(msg: &M) -> Result<Bytes> {
    let body = msg.encode_to_vec();

    if body.len() > MAX_FRAME_SIZE {
        return Err(Error::MessageTooLarge {
            size: body.len(),
            max: MAX_FRAME_SIZE,
        });
    }

    let len = body.len() as u16;
    let mut buf = BytesMut::with_capacity(2 + body.len());
    buf.put_u16_le(len);
    buf.put_slice(&body);

    Ok(buf.freeze())
}

/// Decode a discovery frame.
///
/// Returns the protobuf body bytes.
pub fn decode_frame(data: &[u8]) -> Result<Bytes> {
    if data.len() < 2 {
        return Err(Error::InvalidFrame {
            expected: 2,
            actual: data.len(),
        });
    }

    let mut cursor = std::io::Cursor::new(data);
    let len = cursor.get_u16_le() as usize;

    let remaining = &data[2..];
    if remaining.len() < len {
        return Err(Error::InvalidFrame {
            expected: len,
            actual: remaining.len(),
        });
    }

    Ok(Bytes::copy_from_slice(&remaining[..len]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msgs::Boolean;

    #[test]
    fn test_roundtrip() {
        let msg = Boolean {
            header: None,
            data: true,
        };

        let encoded = encode_frame(&msg).unwrap();
        let body = decode_frame(&encoded).unwrap();

        let decoded = Boolean::decode(body).unwrap();
        assert!(decoded.data);
    }

    #[test]
    fn test_decode_short() {
        assert!(decode_frame(&[]).is_err());
        assert!(decode_frame(&[0]).is_err());
    }
}
