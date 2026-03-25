//! Generated protobuf message types.
//!
//! This module contains all the Gazebo message types compiled from `.proto` files.
//!
//! Note: Prost converts proto message names to Rust conventions:
//! - `Pose_V` becomes `PoseV`
//! - `Vector3d` stays `Vector3d`

// Include generated protobuf code (prost output has no doc comments)
#[allow(missing_docs)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/gz.msgs.rs"));
}
pub use generated::*;

impl Color {
    /// Convert to `[r, g, b, a]` with Gazebo alpha semantics.
    ///
    /// Proto3 defaults `float a` to 0.0, but Gazebo's
    /// `gz::math::Color` defaults alpha to 1.0 (opaque).
    /// Gazebo's own C++ `Convert` functions apply the same
    /// correction. All rendering code should use this
    /// instead of reading `self.a` directly.
    pub fn to_rgba(&self) -> [f32; 4] {
        let a = if self.a == 0.0 { 1.0 } else { self.a };
        [self.r, self.g, self.b, a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_rgba_clamps_default_alpha() {
        // Proto3 default: a=0.0 should become 1.0
        let c = Color {
            r: 0.3,
            g: 0.3,
            b: 0.3,
            a: 0.0,
            ..Default::default()
        };
        assert_eq!(c.to_rgba(), [0.3, 0.3, 0.3, 1.0]);
    }

    #[test]
    fn test_to_rgba_preserves_explicit_alpha() {
        let c = Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 0.5,
            ..Default::default()
        };
        assert_eq!(c.to_rgba(), [1.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn test_to_rgba_preserves_full_opaque() {
        let c = Color {
            r: 0.2,
            g: 0.2,
            b: 0.2,
            a: 1.0,
            ..Default::default()
        };
        assert_eq!(c.to_rgba(), [0.2, 0.2, 0.2, 1.0]);
    }
}
