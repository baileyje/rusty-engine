//! Common component types used across benchmarks.
//!
//! These components are designed to be representative of real game components
//! in terms of size and access patterns.

use rusty_macros::{Component, Unique};

// =============================================================================
// Transform Components (common in most games)
// =============================================================================

/// 3D position component (12 bytes).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// 3D velocity component (12 bytes).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Velocity {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// 3D acceleration component (12 bytes).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Acceleration {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Rotation as euler angles (12 bytes).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Rotation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// 4x4 transformation matrix (64 bytes).
#[derive(Component, Clone, Copy, Debug)]
pub struct Transform {
    pub matrix: [[f32; 4]; 4],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

/// Unique world delta time.
#[derive(Unique)]
pub struct DeltaTime(pub f32);

// =============================================================================
// Game Entity Components
// =============================================================================

/// Health component for damageable entities.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

/// Simple AI state component.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AiState {
    pub state: u32,
    pub timer: f32,
    pub target_x: f32,
    pub target_y: f32,
}

/// Team/faction identifier.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Team {
    pub id: u32,
}

/// A projectile marker
#[derive(Component, Clone)]
pub struct Projectile;

// =============================================================================
// Particle System Components
// =============================================================================

/// A Particle marker
#[derive(Component, Clone)]
pub struct Particle;

/// Particle lifetime tracking.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Lifetime {
    pub remaining: f32,
    pub total: f32,
}

/// RGBA color (16 bytes).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Particle size/scale.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

// =============================================================================
// Fragmentation Test Components (A-Z for archetype fragmentation)
// =============================================================================

/// Shared data component for fragmentation tests.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Data {
    pub value: f64,
}

// Marker components for creating many archetypes
macro_rules! define_marker_components {
    ($($name:ident),*) => {
        $(
            #[derive(Component, Clone, Copy, Debug, Default)]
            pub struct $name;
        )*
    };
}

define_marker_components!(
    MarkerA, MarkerB, MarkerC, MarkerD, MarkerE, MarkerF, MarkerG, MarkerH, MarkerI, MarkerJ,
    MarkerK, MarkerL, MarkerM, MarkerN, MarkerO, MarkerP, MarkerQ, MarkerR, MarkerS, MarkerT,
    MarkerU, MarkerV, MarkerW, MarkerX, MarkerY, MarkerZ
);

// =============================================================================
// Component Size Reference
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn document_component_sizes() {
        // Transform components
        assert_eq!(size_of::<Position>(), 12);
        assert_eq!(size_of::<Velocity>(), 12);
        assert_eq!(size_of::<Acceleration>(), 12);
        assert_eq!(size_of::<Rotation>(), 12);
        assert_eq!(size_of::<Transform>(), 64);

        // Game components
        assert_eq!(size_of::<Health>(), 8);
        assert_eq!(size_of::<AiState>(), 16);
        assert_eq!(size_of::<Team>(), 4);

        // Particle components
        assert_eq!(size_of::<Lifetime>(), 8);
        assert_eq!(size_of::<Color>(), 16);
        assert_eq!(size_of::<Size>(), 8);

        // Data component
        assert_eq!(size_of::<Data>(), 8);

        // Marker components (ZST)
        assert_eq!(size_of::<MarkerA>(), 0);
    }
}
