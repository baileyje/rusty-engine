//! Unique (singleton) parameter types for system functions.
//!
//! This module provides [`Res`] and [`ResMut`] parameter types that allow systems to access
//! global uniques stored in the world. Uniques are singleton values that exist independently
//! of entities.
//!
//! # Overview
//!
//! Uniques are useful for:
//! - **Global game state**: Score, time, settings
//! - **Shared services**: Asset managers, input handlers
//! - **Configuration**: Game rules, physics constants
//!
//! # Unique vs Component Access
//!
//! | Aspect | Uniques | Components |
//! |--------|---------|------------|
//! | Cardinality | One per type | Many per type (one per entity) |
//! | Access pattern | Direct lookup by type | Query over matching entities |
//! | Use case | Global state | Entity-specific data |
//!
//! # Examples
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
//! // Read-only unique access
//! fn print_time(time: Uniq<GameTime>) {
//!     println!("Elapsed: {}", time.elapsed);
//! }
//!
//! // Mutable unique access
//! fn update_score(mut score: UniqMut<Score>) {
//!     score.0 += 10;
//! }
//!
//! // Multiple uniques
//! fn update_with_delta(time: Uniq<GameTime>, mut score: UniqMut<Score>) {
//!     score.0 += (time.delta * 100.0) as u32;
//! }
//!
//! // Optional uniques (don't panic if missing)
//! fn maybe_update(score: Option<UniqMut<Score>>) {
//!     if let Some(mut s) = score {
//!         s.0 += 1;
//!     }
//! }
//!
//! // Combining with queries
//! fn apply_gravity(
//!     time: Uniq<GameTime>,
//!     query: query::Result<&mut Velocity>,
//! ) {
//!     for vel in query {
//!         vel.y -= 9.8 * time.delta;
//!     }
//! }
//! ```
//!
//! # Conflict Detection
//!
//! The scheduler uses access requests to detect conflicts:
//! - Multiple `Uniq<T>` for the same `T`: No conflict (multiple readers OK)
//! - `Uniq<T>` and `UniqMut<T>` for the same `T`: Conflict (reader + writer)
//! - `UniqMut<T>` and `UniqMut<T>` for the same `T`: Conflict (multiple writers)
//! - Different unique types: No conflict
//!
//! # Panics
//!
//! `Uniq<T>` and `UniqMut<T>` will panic if the unique doesn't exist in the world.
//! Use `Option<Uniq<T>>` or `Option<UniqMut<T>>` for uniques that may not be present.

use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use super::Parameter;
use crate::ecs::{unique, world};

/// Immutable access to a unique in the world.
///
/// `Uniq<U>` provides read-only access to a unique of type `U`. Multiple systems can
/// hold `Uniq<U>` for the same unique type simultaneously, as immutable access doesn't
/// conflict.
///
/// # Type Parameter
///
/// - `U`: The unique type, must implement [`Unique`](unique::Unique)
///
/// # Dereferencing
///
/// `Uniq<U>` implements [`Deref`] to `U`, so you can access fields directly:
///
/// ```rust,ignore
/// fn read_time(time: Uniq<GameTime>) {
///     // Direct field access via Deref
///     println!("Delta: {}", time.delta);
///
///     // Or explicit deref
///     let t: &GameTime = &*time;
/// }
/// ```
///
/// # Panics
///
/// Panics during system execution if the unique doesn't exist. Use [`Option<Uniq<U>>`]
/// if the unique may not be present.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Unique)]
/// struct Config {
///     gravity: f32,
///     max_speed: f32,
/// }
///
/// fn physics_system(config: Uniq<Config>, query: query::Result<&mut Velocity>) {
///     for vel in query {
///         vel.y -= config.gravity;
///         vel.y = vel.y.min(config.max_speed);
///     }
/// }
/// ```
pub struct Uniq<'w, U: unique::Unique> {
    value: &'w U,
    _marker: PhantomData<U>,
}

impl<'w, U: unique::Unique> Uniq<'w, U> {
    /// Creates a new `Res` wrapper around a unique reference.
    ///
    /// This is typically called by the parameter extraction system, not directly by users.
    #[inline]
    pub fn new(value: &'w U) -> Self {
        Self {
            value,
            _marker: PhantomData,
        }
    }
}

impl<U: unique::Unique> Deref for Uniq<'_, U> {
    type Target = U;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

/// Implementation of [`Parameter`] for immutable unique access.
///
/// This allows systems to declare read-only dependencies on uniques.
///
/// # Access Request
///
/// Returns immutable access to the unique type `U`. The scheduler uses this to:
/// - Allow multiple systems with `Uniq<U>` to run in parallel
/// - Prevent `Uniq<U>` from running with `UniqMut<U>` for the same type
///
/// # Implementation Details
///
/// - **Value type**: `Uniq<'w, U>` where `'w` is the shard lifetime
/// - **State**: None required (unit type)
/// - **Extraction**: Calls `shard.get_unique::<U>()` and wraps in `Res`
impl<U: unique::Unique> Parameter for Uniq<'_, U> {
    /// The value type is the `Res` wrapper with world lifetime.
    type Value<'w, 's> = Uniq<'w, U>;

    /// No state needed for unique parameters.
    type State = ();

    /// Uniques don't require build-time state.
    fn build_state(_world: &mut world::World) -> Self::State {}

    /// Returns immutable access request for unique `U`.
    fn required_access(world: &world::World) -> world::AccessRequest {
        world::AccessRequest::to_resources(&[world.resources().register_unique::<U>()], &[])
    }

    /// Extracts the unique from the shard.
    ///
    /// # Panics
    ///
    /// Panics if the unique doesn't exist in the world.
    ///
    /// # Safety
    ///
    /// Caller must ensure the shard's grant includes access to this unique.
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        shard
            .get_unique::<U>()
            .map(Uniq::new)
            .expect("Unique not found")
    }
}

/// Mutable access to a unique in the world.
///
/// `UniqMut<U>` provides read-write access to a unique of type `U`. Only one system can
/// hold `UniqMut<U>` for a given unique type at a time, as mutable access is exclusive.
///
/// # Type Parameter
///
/// - `U`: The unique type, must implement [`Unique`](unique::Unique)
///
/// # Dereferencing
///
/// `UniqMut<U>` implements both [`Deref`] and [`DerefMut`] to `U`, so you can access
/// and modify fields directly:
///
/// ```rust,ignore
/// fn update_time(mut time: UniqMut<GameTime>) {
///     // Direct field access via Deref
///     println!("Current delta: {}", time.delta);
///
///     // Mutation via DerefMut
///     time.elapsed += time.delta;
/// }
/// ```
///
/// # Panics
///
/// Panics during system execution if the unique doesn't exist. Use [`Option<UniqMut<U>>`]
/// if the unique may not be present.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Unique)]
/// struct Score(u32);
///
/// fn add_points(mut score: UniqMut<Score>, query: query::Result<&Enemy>) {
///     for _ in query {
///         score.0 += 10;
///     }
/// }
/// ```
pub struct UniqMut<'w, U: unique::Unique> {
    value: &'w mut U,
    _marker: PhantomData<U>,
}

impl<'w, U: unique::Unique> UniqMut<'w, U> {
    /// Creates a new `ResMut` wrapper around a mutable unique reference.
    ///
    /// This is typically called by the parameter extraction system, not directly by users.
    #[inline]
    pub fn new(value: &'w mut U) -> Self {
        Self {
            value,
            _marker: PhantomData,
        }
    }
}

impl<U: unique::Unique> Deref for UniqMut<'_, U> {
    type Target = U;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<U: unique::Unique> DerefMut for UniqMut<'_, U> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

/// Implementation of [`Parameter`] for mutable unique access.
///
/// This allows systems to declare read-write dependencies on uniques.
///
/// # Access Request
///
/// Returns mutable access to the unique type `U`. The scheduler uses this to:
/// - Prevent any other system from accessing `U` while this system runs
/// - Ensure exclusive access to the unique
///
/// # Implementation Details
///
/// - **Value type**: `UniqMut<'w, U>` where `'w` is the shard lifetime
/// - **State**: None required (unit type)
/// - **Extraction**: Calls `shard.get_unique_mut::<U>()` and wraps in `ResMut`
impl<U: unique::Unique> Parameter for UniqMut<'_, U> {
    /// The value type is the `ResMut` wrapper with world lifetime.
    type Value<'w, 's> = UniqMut<'w, U>;

    /// No state needed for unique parameters.
    type State = ();

    /// Uniques don't require build-time state.
    fn build_state(_world: &mut world::World) -> Self::State {}

    /// Returns mutable access request for unique `U`.
    fn required_access(world: &world::World) -> world::AccessRequest {
        world::AccessRequest::to_resources(&[], &[world.resources().register_unique::<U>()])
    }

    /// Extracts the unique mutably from the shard.
    ///
    /// # Panics
    ///
    /// Panics if the unique doesn't exist in the world.
    ///
    /// # Safety
    ///
    /// Caller must ensure the shard's grant includes mutable access to this unique.
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        shard
            .get_unique_mut::<U>()
            .map(UniqMut::new)
            .expect("Unique not found")
    }
}

/// Implementation of [`Parameter`] for optional immutable unique access.
///
/// `Option<Uniq<U>>` allows systems to gracefully handle missing uniques instead of panicking.
/// This is useful for uniques that may not always be present in the world.
///
/// # When to Use
///
/// - Uniques that are conditionally inserted (e.g., debug overlays, optional features)
/// - Systems that should run even without the unique
/// - Startup systems that may run before uniques are initialized
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Unique)]
/// struct DebugOverlay {
///     enabled: bool,
/// }
///
/// fn render_debug(overlay: Option<Uniq<DebugOverlay>>, query: query::Result<&Position>) {
///     // Only render debug info if the overlay unique exists and is enabled
///     if let Some(overlay) = overlay {
///         if overlay.enabled {
///             for pos in query {
///                 println!("Entity at ({}, {})", pos.x, pos.y);
///             }
///         }
///     }
/// }
/// ```
///
/// # Access Request
///
/// Still requests immutable access to `U`. The access is needed to safely read the unique
/// if it exists, even though the system handles its absence gracefully.
impl<U: unique::Unique> Parameter for Option<Uniq<'_, U>> {
    type Value<'w, 's> = Option<Uniq<'w, U>>;
    type State = ();

    fn build_state(_world: &mut world::World) -> Self::State {}

    fn required_access(world: &world::World) -> world::AccessRequest {
        world::AccessRequest::to_resources(&[world.resources().register_unique::<U>()], &[])
    }

    /// Extracts the unique if it exists, returning `None` otherwise.
    ///
    /// Unlike `Uniq<U>`, this never panics on missing uniques.
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        shard.get_unique::<U>().map(Uniq::new)
    }
}

/// Implementation of [`Parameter`] for optional mutable unique access.
///
/// `Option<UniqMut<U>>` allows systems to gracefully handle missing uniques while still
/// being able to mutate them if present.
///
/// # When to Use
///
/// - Uniques that may be dynamically added/removed during runtime
/// - Systems that optionally modify state if available
/// - Feature flags or optional game modes
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Unique)]
/// struct Multiplier(f32);
///
/// fn apply_bonus(
///     mut multiplier: Option<UniqMut<Multiplier>>,
///     query: query::Result<&mut Score>,
/// ) {
///     let mult = multiplier.as_mut().map(|m| m.0).unwrap_or(1.0);
///     for score in query {
///         score.0 = (score.0 as f32 * mult) as u32;
///     }
/// }
/// ```
///
/// # Access Request
///
/// Still requests mutable access to `U`. The exclusive access is needed to safely
/// modify the unique if it exists.
impl<U: unique::Unique> Parameter for Option<UniqMut<'_, U>> {
    type Value<'w, 's> = Option<UniqMut<'w, U>>;
    type State = ();

    fn build_state(_world: &mut world::World) -> Self::State {}

    fn required_access(world: &world::World) -> world::AccessRequest {
        world::AccessRequest::to_resources(&[], &[world.resources().register_unique::<U>()])
    }

    /// Extracts the unique mutably if it exists, returning `None` otherwise.
    ///
    /// Unlike `UniqMut<U>`, this never panics on missing uniques.
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        shard.get_unique_mut::<U>().map(UniqMut::new)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::ecs::{system::Parameter, world};
    use rusty_macros::Unique;

    #[derive(Unique)]
    struct Res1 {
        num: i32,
    }

    fn test_setup() -> world::World {
        let mut world = world::World::new(world::Id::new(0));
        world.add_unique(Res1 { num: 100 });
        world
    }

    #[test]
    fn res_param_access() {
        // Given
        let world = test_setup();

        // When
        let access = <Uniq<Res1> as Parameter>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(&[world.resources().get::<Res1>().unwrap()], &[])
        )
    }

    #[test]
    fn res_param_get() {
        // Given
        let mut world = test_setup();
        let mut state: () = <Uniq<Res1> as Parameter>::build_state(&mut world);
        let access = <Uniq<Res1> as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let result = unsafe { <Uniq<Res1> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.num, 100);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn opt_res_param_access() {
        // Given
        let world = test_setup();

        // When
        let access = <Option<Uniq<Res1>> as Parameter>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(&[world.resources().get::<Res1>().unwrap()], &[])
        )
    }

    #[test]
    fn opt_res_param_get() {
        // Given
        let mut world = test_setup();
        let mut state: () = <Option<Uniq<Res1>> as Parameter>::build_state(&mut world);
        let access = <Option<Uniq<Res1>> as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let result = unsafe { <Option<Uniq<Res1>> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.num, 100);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn opt_res_param_get_none() {
        // Given
        let mut world = test_setup();

        #[derive(Unique)]
        struct NotFound;

        let mut state: () = <Option<Uniq<NotFound>> as Parameter>::build_state(&mut world);
        let access = <Option<Uniq<NotFound>> as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let result = unsafe { <Option<Uniq<NotFound>> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert!(result.is_none());

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn res_mut_param_access() {
        // Given
        let world = test_setup();

        // When
        let access = <UniqMut<Res1> as Parameter>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(&[], &[world.resources().get::<Res1>().unwrap()])
        )
    }

    #[test]
    fn res_mut_param_get() {
        // Given
        let mut world = test_setup();
        let mut state: () = <UniqMut<Res1> as Parameter>::build_state(&mut world);
        let access = <UniqMut<Res1> as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result = unsafe { <UniqMut<Res1> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.num, 100);

        // When
        result.num += 50;

        // Then
        let rsult = world.get_unique::<Res1>().unwrap();
        assert_eq!(rsult.num, 150);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn opt_res_mut_param_access() {
        // Given
        let world = test_setup();

        // When
        let access = <Option<UniqMut<Res1>> as Parameter>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(&[], &[world.resources().get::<Res1>().unwrap()])
        )
    }

    #[test]
    fn opt_res_mut_param_get() {
        // Given
        let mut world = test_setup();
        let mut state: () = <Option<UniqMut<Res1>> as Parameter>::build_state(&mut world);
        let access = <Option<UniqMut<Res1>> as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let result = unsafe { <Option<UniqMut<Res1>> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert!(result.is_some());
        let mut result = result.unwrap();
        assert_eq!(result.num, 100);

        // When
        result.num += 50;

        // Then
        let rsult = world.get_unique::<Res1>().unwrap();
        assert_eq!(rsult.num, 150);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn opt_res_mut_param_get_none() {
        // Given
        let mut world = test_setup();

        #[derive(Unique)]
        struct NotFound;

        let mut state: () = <Option<UniqMut<NotFound>> as Parameter>::build_state(&mut world);
        let access = <Option<UniqMut<NotFound>> as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let result =
            unsafe { <Option<UniqMut<NotFound>> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert!(result.is_none());

        // Release shard
        world.release_shard(shard);
    }
}
