//! Component management for the ECS.
//!
//! This module provides the infrastructure for registering, storing, and managing component types
//! in the Entity Component System. Components are the data containers that can be attached to
//! entities to give them properties and behaviors.
//!
//! ## Architecture
//!
//! The component system consists of several key types:
//!
//! - [`Component`]: The trait that all component types must implement
//! - [`Id`]: A unique identifier for each registered component type
//! - [`Registry`]: Thread-safe registration and lookup of component types
//! - [`Info`]: Metadata about a component type (layout, drop function, etc.)
//! - [`Spec`]: A specification describing a set of component types
//! - [`Set`]: A trait for applying component values to entities
//!
//! ## Thread Safety
//!
//! The [`Registry`] is designed for high-performance concurrent access:
//! - Lock-free reads for component ID lookups using `DashMap`
//! - Minimal locking for registration (only when a new type is first registered)
//! - Component registration is idempotent and thread-safe
//!
//! ## Usage
//!
//! ```ignore
//! use rusty_engine::ecs::component::{Component, Registry};
//!
//! #[derive(Component)]
//! struct Position { x: f32, y: f32 }
//!
//! let registry = Registry::new();
//! let pos_id = registry.register::<Position>();
//! ```

use std::hash::Hash;

mod info;
mod registry;
mod set;
mod spec;

pub(crate) use info::Info;
pub use registry::Registry;
use rusty_macros::Component;
pub(crate) use set::{Set, Target as SetTarget};
pub(crate) use spec::Spec;

/// A component identifier. This is a non-zero unique identifier for a component type in the ECS.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32);

impl Id {
    /// Get the index of this component if it were to live in indexable storage (e.g. Vec)
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
}

/// A trait representing a component in the ECS (Entity Component System).
///
/// At present this only sets the required trait bounds for a type to be used as a component.
///
/// Eventually this may be expanded to include common functionality for components.
pub trait Component: 'static + Sized + Send + Sync {}
