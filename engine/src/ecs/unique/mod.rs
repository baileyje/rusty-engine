//! Unique (singleton) types for the ECS.
//!
//! This module provides the [`Unique`] trait for types that exist as singletons
//! in the world - exactly one instance per type, accessible globally.
//!
//! # Unique vs Component
//!
//! | Aspect | Unique | Component |
//! |--------|--------|-----------|
//! | Cardinality | One per type per world | Many per type (one per entity) |
//! | Access | Direct by type | Query over matching entities |
//! | Use case | Global state, services | Entity-specific data |
//!
//! # Example
//!
//! ```rust,ignore
//! use rusty_macros::Unique;
//!
//! #[derive(Unique)]
//! struct GameTime {
//!     elapsed: f32,
//!     delta: f32,
//! }
//!
//! #[derive(Unique)]
//! struct Score(u32);
//!
//! // Add to world
//! world.add_unique(GameTime { elapsed: 0.0, delta: 0.016 });
//! world.add_unique(Score(0));
//!
//! // Access in systems via Res/ResMut parameters
//! fn update_score(time: Res<GameTime>, mut score: ResMut<Score>) {
//!     score.0 += (time.delta * 100.0) as u32;
//! }
//! ```

/// A trait for singleton types in the ECS.
///
/// Types implementing `Unique` can be stored once per world and accessed
/// globally via `Res<T>` and `ResMut<T>` system parameters.
///
/// # Derive Macro
///
/// Use `#[derive(Unique)]` to implement this trait:
///
/// ```rust,ignore
/// #[derive(Unique)]
/// struct GameTime {
///     elapsed: f32,
///     delta: f32,
/// }
/// ```
///
/// # Trait Bounds
///
/// - `'static`: No borrowed data
/// - `Send + Sync`: Safe to share across threads
pub trait Unique: 'static + Send + Sync {}
