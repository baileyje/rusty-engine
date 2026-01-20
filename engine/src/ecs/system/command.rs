//! Deferred command buffer for entity and component operations.
//!
//! This module provides a thread-safe command buffer that allows systems to queue
//! entity operations (spawn, despawn, add/remove components) without requiring
//! exclusive world access. Commands are collected during system execution and
//! applied to the world at designated flush points.
//!
//! # Overview
//!
//! The command buffer solves a fundamental ECS challenge: systems running in parallel
//! cannot directly mutate world structure (spawning/despawning entities, adding/removing
//! components) because these operations require exclusive access. Instead, systems
//! submit commands to a shared buffer, which is flushed between execution phases.
//!
//! # Thread Safety
//!
//! - [`CommandBuffer::push`] is lock-free and wait-free for producers
//! - Multiple systems can safely push commands concurrently
//! - [`CommandBuffer::flush`] must be called from a single thread with `&mut World`
//!
//! # Lifecycle
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Schedule Phase                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │  System A ──push──┐                                         │
//! │  System B ──push──┼──► CommandBuffer ──flush──► World       │
//! │  System C ──push──┘         ▲                               │
//! │                             │                               │
//! │                    (between phases)                         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use rusty_engine::ecs::system::{Commands, CommandBuffer};
//!
//! // In a system, use the Commands parameter
//! fn spawner(mut commands: Commands) {
//!     let entity = commands.spawn((Position { x: 0.0, y: 0.0 }, Velocity::default()));
//!     commands.add_components(entity, Health { value: 100 });
//! }
//!
//! // After phase execution, flush commands to the world
//! command_buffer.flush(&mut world);
//! ```

use crossbeam::queue::SegQueue;

use crate::ecs::{component, entity, world};

/// A deferred entity command.
///
/// Commands are queued during system execution and applied to the world
/// at flush time. Each variant represents a different structural operation.
pub enum Command {
    /// Spawn a new entity with the given components.
    ///
    /// The entity ID is pre-allocated when the command is created,
    /// allowing systems to reference the entity before it exists in storage.
    Spawn {
        /// The pre-allocated entity ID.
        entity: entity::Entity,
        /// Type-erased component values to attach.
        components: component::BoxedSet,
    },

    /// Remove an entity and all its components from the world.
    ///
    /// The entity's ID will be returned to the allocator's dead pool
    /// with an incremented generation for future reuse.
    Despawn {
        /// The entity to remove.
        entity: entity::Entity,
    },

    /// Add components to an existing entity.
    ///
    /// If the entity already has any of the components, they will be replaced.
    /// This may cause the entity to migrate to a different archetype table.
    AddComponents {
        /// The target entity.
        entity: entity::Entity,
        /// Type-erased component values to add.
        components: component::BoxedSet,
    },

    /// Remove components from an existing entity by type.
    ///
    /// Components not present on the entity are silently ignored.
    /// This may cause the entity to migrate to a different archetype table.
    RemoveComponents {
        /// The target entity.
        entity: entity::Entity,
        /// Specification of which component types to remove.
        spec: component::Spec,
    },
}

/// Thread-safe command buffer using a lock-free queue.
///
/// The buffer collects deferred entity commands from parallel systems
/// and applies them to the world at designated flush points.
///
/// # Thread Safety
///
/// - `push()` is lock-free and can be called from multiple threads
/// - `drain()` and `flush()` should be called from a single thread
///
/// # Performance
///
/// Uses `crossbeam::queue::SegQueue` internally, which provides:
/// - Wait-free push operations
/// - Lock-free pop operations
/// - Good cache locality for batched operations
#[derive(Default)]
pub struct CommandBuffer {
    commands: SegQueue<Command>,
}

impl CommandBuffer {
    /// Create a new empty command buffer.
    pub fn new() -> Self {
        Self {
            commands: SegQueue::new(),
        }
    }

    /// Push a command to the buffer.
    ///
    /// This operation is lock-free and wait-free, making it safe to call
    /// from multiple threads concurrently without blocking.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// buffer.push(Command::Despawn { entity });
    /// ```
    pub fn push(&self, command: Command) {
        self.commands.push(command);
    }

    /// Drain all commands from the buffer.
    ///
    /// Returns a `Vec` containing all queued commands in FIFO order.
    /// The buffer is empty after this call.
    ///
    /// # Note
    ///
    /// This should be called from a single thread. While technically safe
    /// to call concurrently, doing so would split commands unpredictably.
    pub fn drain(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        while let Some(cmd) = self.commands.pop() {
            commands.push(cmd);
        }
        commands
    }

    /// Flush all queued commands to the world.
    ///
    /// Drains the buffer and applies each command to the world in order.
    /// This is typically called between schedule phases.
    ///
    /// # Command Execution Order
    ///
    /// Commands are applied in FIFO order (first pushed, first executed).
    /// This means:
    /// - An entity spawned before being despawned will exist briefly
    /// - Components added then removed will not be present after flush
    ///
    /// # Panics
    ///
    /// May panic if:
    /// - A despawn targets a non-existent entity
    /// - Component operations target an invalid entity
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // After all systems in a phase have run
    /// command_buffer.flush(&mut world);
    /// ```
    pub fn flush(&self, world: &mut world::World) {
        for command in self.drain() {
            match command {
                Command::Spawn { entity, components } => {
                    world.spawn_dynamic(entity, components);
                }
                Command::Despawn { entity } => {
                    world.despawn(entity);
                }
                Command::AddComponents { entity, components } => {
                    world.add_components(entity, components);
                }
                Command::RemoveComponents { entity, spec } => {
                    world.remove_components_dynamic(entity, &spec);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use super::*;

    #[derive(Component)]
    #[allow(dead_code)]
    struct Position {
        x: u8,
        y: u8,
    }

    #[derive(Component)]
    #[allow(dead_code)]
    struct Velocity {
        dx: u8,
        dy: u8,
    }

    #[test]
    pub fn basic_spawn_buffering() {
        // Given
        let allocator = entity::Allocator::new();
        let types = world::TypeRegistry::new();
        let buffer = CommandBuffer::new();

        // When
        buffer.push(Command::Spawn {
            entity: allocator.alloc(),
            components: component::BoxedSet::new(
                (Position { x: 1, y: 2 }, Velocity { dx: 0, dy: 1 }),
                &types,
            ),
        });

        // Then
        // let cmd = buffer.drain().first();
        // assert!(cmd.is_some());
    }
}
