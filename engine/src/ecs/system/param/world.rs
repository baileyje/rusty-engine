use super::Parameter;
use crate::ecs::world;

/// Implementation of [`Parameter`] for direct immutable world access.
///
/// This allows systems to read world structure (entity metadata, etc.) directly.
/// Note that this parameter type can only be used in exclusive systems (those taking
/// `&mut World` directly), not in parallel systems with multiple parameters.
///
/// # Scheduling Implications
///
/// Immutable world access indicates the system needs read-only access to the world structure
/// itself (not components). A scheduler should treat this as exclusive access for safety.
///
/// # When to Use
///
/// Use immutable world access when you need to:
/// - **Access entity metadata**: Query entity existence, generation counters, etc.
/// - **Read world configuration**: World ID or other world-level data
///
/// Don't use world access for:
/// - **Reading components**: Use queries instead for better performance
/// - **Parallel systems**: World parameters bypass shard grants
///
/// # Examples
///
/// ```rust,ignore
/// // Exclusive system with world access
/// fn validator(world: &World) {
///     for entity in some_entity_list {
///         if world.entity(entity).is_none() {
///             println!("Entity no longer exists!");
///         }
///     }
/// }
/// ```
///
/// # Implementation Details
///
/// - **Value type**: `&'w World` where `'w` is the shard lifetime
/// - **Access request**: Returns access request for immutable world access
/// - **Extraction**: Extracts world from shard unsafely (bypassing grant)
///
impl Parameter for &world::World {
    /// The value type is an immutable world reference with shard lifetime.
    type Value<'w, 's> = &'w world::World;

    /// The state type is empty since no state is needed for immutable world access.
    type State = ();

    /// Build empty state for this parameter.
    fn build_state(_world: &mut world::World) -> Self::State {}

    /// Get the world access request for this world parameter.
    fn required_access(_world: &world::World) -> world::AccessRequest {
        world::AccessRequest::to_world(false)
    }

    /// Get immutable access to the world from the shard.
    ///
    /// # Safety
    ///
    /// This bypasses the shard's grant checking and accesses the world directly.
    /// It should only be used in systems that have been validated to require
    /// world-level access. Typically, world parameters indicate the system should
    /// be exclusive rather than parallel.
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        // SAFETY: Caller ensures this system has exclusive world access rights
        unsafe { shard.world() }
    }
}

#[cfg(test)]
mod tests {

    use crate::ecs::{system::Parameter, world};

    #[test]
    fn world_param_component_spec() {
        // Given
        let world = world::World::new(world::Id::new(0));

        // When
        let access = <&world::World as Parameter>::required_access(&world);

        // Then
        assert!(access.world());
    }

    #[test]
    fn world_param_get() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        #[allow(clippy::let_unit_value)]
        let mut state = <&world::World as Parameter>::build_state(&mut world);
        let access = <&world::World as Parameter>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let world_ref = unsafe { <&world::World as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(world_ref.id(), world.id());

        // Release shard
        world.release_shard(shard);
    }
}
