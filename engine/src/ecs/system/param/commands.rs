//! Commands system parameter for deferred entity operations.
//!
//! The [`Commands`] parameter allows systems to queue structural changes
//! (spawning, despawning, component modifications) without exclusive world access.

use crate::ecs::{
    component, entity,
    system::{
        Parameter,
        command::{Command, CommandBuffer},
    },
    world,
};

/// System parameter for submitting deferred entity commands.
///
/// `Commands` provides a safe interface for systems to queue structural world changes
/// that will be applied at the next flush point (typically between schedule phases).
///
/// # Why Deferred?
///
/// Systems running in parallel cannot directly modify world structure because:
/// - Spawning/despawning changes archetype tables
/// - Adding/removing components may relocate entities
/// - These operations require exclusive `&mut World` access
///
/// Instead, systems push commands to a shared buffer, which is flushed when safe.
///
/// # Entity IDs
///
/// When spawning, an entity ID is allocated immediately and returned. This ID
/// can be used to reference the entity in subsequent commands within the same
/// system, even though the entity won't exist in storage until flush.
///
/// ```rust,ignore
/// fn setup(commands: Commands) {
///     // Entity ID is valid immediately for referencing
///     let parent = commands.spawn(Parent);
///     let child = commands.spawn((Child, ParentRef(parent)));
///     // After flush, both entities exist with correct references
/// }
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use rusty_engine::ecs::system::Commands;
///
/// fn spawner(commands: Commands) {
///     // Spawn with single component
///     commands.spawn(Position { x: 0.0, y: 0.0 });
///
///     // Spawn with component tuple
///     let entity = commands.spawn((
///         Position { x: 1.0, y: 2.0 },
///         Velocity { dx: 0.5, dy: 0.0 },
///     ));
///
///     // Modify existing entity
///     commands.add_components(entity, Health { value: 100 });
///
///     // Remove components by type
///     commands.remove_components::<Velocity>(entity);
///
///     // Despawn
///     commands.despawn(entity);
/// }
/// ```
///
/// # Thread Safety
///
/// `Commands` itself is not `Send` or `Sync`, but the underlying buffer is.
/// Each system receives its own `Commands` instance pointing to the shared buffer.
pub struct Commands<'a> {
    buffer: &'a CommandBuffer,
    allocator: &'a entity::Allocator,
    registry: &'a world::TypeRegistry,
}

impl<'a> Commands<'a> {
    /// Spawn a new entity with the given components.
    ///
    /// Returns an entity ID that is valid immediately for referencing,
    /// though the entity won't exist in world storage until flush.
    ///
    /// # Type Parameters
    ///
    /// - `S`: Any type implementing [`component::Set`] (single component or tuple)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Single component
    /// let e1 = commands.spawn(Position { x: 0.0, y: 0.0 });
    ///
    /// // Multiple components as tuple
    /// let e2 = commands.spawn((Position::default(), Velocity::default(), Health { value: 100 }));
    ///
    /// // Reference spawned entity in another command
    /// commands.add_components(e1, Target(e2));
    /// ```
    pub fn spawn<S: component::Set + Send>(&self, values: S) -> entity::Entity {
        let entity = self.allocator.alloc();
        self.buffer.push(Command::Spawn {
            entity,
            components: component::BoxedSet::new(values, self.registry),
        });
        entity
    }

    /// Queue an entity for removal.
    ///
    /// The entity and all its components will be removed at flush time.
    /// The entity ID will be recycled with an incremented generation.
    ///
    /// # Note
    ///
    /// Despawning an already-despawned or non-existent entity may panic at flush time.
    pub fn despawn(&self, entity: entity::Entity) {
        self.buffer.push(Command::Despawn { entity });
    }

    /// Add components to an existing entity.
    ///
    /// If the entity already has any of the specified components, they will
    /// be replaced with the new values. This may cause the entity to migrate
    /// to a different archetype table.
    ///
    /// # Type Parameters
    ///
    /// - `S`: Any type implementing [`component::Set`] (single component or tuple)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Add single component
    /// commands.add_components(entity, Poisoned { duration: 5.0 });
    ///
    /// // Add multiple components
    /// commands.add_components(entity, (Shield { strength: 50 }, Invulnerable));
    /// ```
    pub fn add_components<S: component::Set + Send>(&self, entity: entity::Entity, values: S) {
        self.buffer.push(Command::AddComponents {
            entity,
            components: component::BoxedSet::new(values, self.registry),
        });
    }

    /// Remove components from an entity by type.
    ///
    /// Components not present on the entity are silently ignored.
    /// This may cause the entity to migrate to a different archetype table.
    ///
    /// # Type Parameters
    ///
    /// - `S`: Any type implementing [`component::IntoSpec`] (single component type or tuple of types)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Remove single component type
    /// commands.remove_components::<Poisoned>(entity);
    ///
    /// // Remove multiple component types
    /// commands.remove_components::<(Shield, Invulnerable)>(entity);
    /// ```
    pub fn remove_components<S: component::IntoSpec>(&self, entity: entity::Entity) {
        self.buffer.push(Command::RemoveComponents {
            entity,
            spec: S::into_spec(self.registry),
        });
    }
}

impl Parameter for Commands<'_> {
    /// Commands with the world's lifetime applied.
    type Value<'w, 's> = Commands<'w>;

    /// No persistent state needed.
    type State = ();

    fn build_state(_world: &mut world::World) -> Self::State {}

    /// Commands require no component access.
    ///
    /// The command buffer and allocator are accessed through separate,
    /// non-conflicting references, so no access request is needed.
    fn required_access(_world: &world::World) -> world::AccessRequest {
        world::AccessRequest::NONE
    }

    /// Extract a Commands handle from the shard.
    ///
    /// # Safety
    ///
    /// This is always safe because:
    /// - The command buffer is a separate, lock-free data structure
    /// - The allocator access doesn't conflict with component access
    /// - The type registry is read-only
    unsafe fn extract<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
        command_buffer: &'w CommandBuffer,
    ) -> Self::Value<'w, 's> {
        Commands {
            buffer: command_buffer,
            allocator: shard.entity_allocator(),
            registry: shard.resources(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ecs::system::Parameter;

    use super::*;

    #[test]
    fn test_commands_param_access() {
        // Given
        let world = world::World::new(world::Id::new(0));

        // When
        let access = <Commands>::required_access(&world);

        // Then
        assert!(access.is_none());
    }

    #[test]
    fn test_commands_param_get() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        #[allow(clippy::let_unit_value)]
        let mut state = <Commands>::build_state(&mut world);
        let access = <Commands>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");
        let command_buffer = CommandBuffer::new();

        // When
        unsafe { <Commands>::extract(&mut shard, &mut state, &command_buffer) };
    }
}
