//! Central registry and manager for all event streams.
//!
//! This module provides [`Broker`], which owns and manages all event streams
//! in the ECS world. It handles stream registration, typed access, and coordinated
//! buffer swapping across all event types.
//!
//! # Overview
//!
//! The `Broker` serves as the central hub for the event system:
//! - **Registration**: Event types must be registered before use
//! - **Type-safe access**: Streams are accessed by their event type
//! - **Coordinated swapping**: All streams swap buffers together via [`swap_all()`](Broker::swap_all)
//!
//! # Type Erasure
//!
//! Internally, the broker stores streams as `Box<dyn ErasedStream>` keyed by `TypeId`.
//! This allows heterogeneous storage while maintaining type safety through generic
//! accessor methods that downcast to the concrete `Stream<E>`.
//!
//! # Integration with World
//!
//! The `Broker` is owned by the [`World`](crate::ecs::world::World) and accessed through:
//! - `world.register_event::<E>()` - Register event types
//! - `world.swap_event_buffers()` - Swap all buffers (call once per frame)
//! - `world.events()` / `world.events_mut()` - Direct broker access
//!
//! # Example
//!
//! ```rust,ignore
//! use rusty_engine::ecs::event::{Event, Broker};
//!
//! #[derive(Clone, Debug)]
//! struct DamageEvent { amount: u32 }
//! impl Event for DamageEvent {}
//!
//! let mut broker = Broker::new();
//!
//! // Register event type (typically done during setup)
//! broker.register::<DamageEvent>();
//!
//! // Send events (typically done by systems)
//! broker.stream_mut::<DamageEvent>()
//!     .unwrap()
//!     .send(DamageEvent { amount: 50 });
//!
//! // Swap buffers (typically done by game loop)
//! broker.swap_all();
//!
//! // Read events (typically done by systems next frame)
//! for event in broker.stream::<DamageEvent>().unwrap().iter() {
//!     println!("Damage: {}", event.amount);
//! }
//! ```

use std::{any::TypeId, collections::HashMap};

use crate::ecs::event::{Event, Stream, stream::ErasedStream};

/// Central registry and manager for all event streams.
///
/// `Broker` owns all [`Stream`] instances and provides:
/// - Type-safe registration and access to streams
/// - Coordinated buffer swapping across all event types
/// - Type erasure for heterogeneous stream storage
///
/// # Registration
///
/// Event types must be registered before they can be used. Registration creates
/// an [`Stream`] with the specified capacity (or default of 1024).
///
/// ```rust,ignore
/// let mut broker = Broker::new();
///
/// // Register with default capacity (1024)
/// broker.register::<CollisionEvent>();
///
/// // Register with custom capacity for high-frequency events
/// broker.register_with_capacity::<ParticleEvent>(4096);
/// ```
///
/// # Thread Safety
///
/// `Broker` itself is not thread-safe. The scheduler's access control system
/// ensures safe concurrent access by:
/// - Granting exclusive access to writers (`Producer<E>`)
/// - Granting shared access to readers (`Consumer<E>`)
/// - Ensuring writers and readers access different buffers
pub struct Broker {
    /// Type-erased storage for event streams, keyed by event TypeId.
    streams: HashMap<TypeId, Box<dyn ErasedStream>>,
}

impl Broker {
    /// Creates a new, empty event broker.
    ///
    /// No event types are registered initially. Call [`register()`](Self::register)
    /// or [`register_with_capacity()`](Self::register_with_capacity) to add event types.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut broker = Broker::new();
    /// assert!(!broker.is_registered::<MyEvent>());
    /// ```
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    /// Registers an event type with the default capacity of 1024.
    ///
    /// This is a convenience method equivalent to `register_with_capacity::<E>(1024)`.
    /// Use this for typical event types; use [`register_with_capacity()`](Self::register_with_capacity)
    /// for high-frequency events that may exceed the default.
    ///
    /// # Panics
    ///
    /// Panics if the event type is already registered.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// broker.register::<PlayerJoinedEvent>();
    /// broker.register::<PlayerLeftEvent>();
    /// ```
    pub fn register<E: Event>(&mut self) {
        self.register_with_capacity::<E>(1024);
    }

    /// Registers an event type with a custom capacity.
    ///
    /// The capacity determines the maximum number of events that can be sent
    /// per frame. Choose capacity based on expected event volume:
    ///
    /// - **Low frequency** (player actions, achievements): 64-256
    /// - **Medium frequency** (collisions, damage): 1024 (default)
    /// - **High frequency** (particles, audio): 4096+
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum events per frame for this event type
    ///
    /// # Panics
    ///
    /// Panics if the event type is already registered.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // High-frequency particle events need larger capacity
    /// broker.register_with_capacity::<ParticleSpawnEvent>(8192);
    /// ```
    pub fn register_with_capacity<E: Event>(&mut self, capacity: usize) {
        let type_id = TypeId::of::<E>();
        assert!(
            !self.streams.contains_key(&type_id),
            "Event type already registered: {:?}",
            std::any::type_name::<E>()
        );
        self.streams
            .insert(type_id, Box::new(Stream::<E>::new(capacity)));
    }

    /// Returns `true` if the event type is registered.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if !broker.is_registered::<MyEvent>() {
    ///     broker.register::<MyEvent>();
    /// }
    /// ```
    #[inline]
    pub fn is_registered<E: Event>(&self) -> bool {
        self.streams.contains_key(&TypeId::of::<E>())
    }

    /// Returns a reference to the event stream for reading.
    ///
    /// Use this to iterate over events in the stable buffer. Events are readable
    /// after [`swap_all()`](Self::swap_all) has been called.
    ///
    /// # Returns
    ///
    /// - `Some(&Stream<E>)` if the event type is registered
    /// - `None` if the event type is not registered
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(stream) = broker.stream::<DamageEvent>() {
    ///     for event in stream.iter() {
    ///         println!("Damage: {}", event.amount);
    ///     }
    /// }
    /// ```
    pub fn stream<E: Event>(&self) -> Option<&Stream<E>> {
        let stream = self.streams.get(&TypeId::of::<E>())?;
        stream.as_any().downcast_ref::<Stream<E>>()
    }

    /// Returns a mutable reference to the event stream for writing.
    ///
    /// Use this to send events to the active buffer. Events become readable
    /// after the next [`swap_all()`](Self::swap_all) call.
    ///
    /// # Returns
    ///
    /// - `Some(&mut Stream<E>)` if the event type is registered
    /// - `None` if the event type is not registered
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(stream) = broker.stream_mut::<DamageEvent>() {
    ///     stream.send(DamageEvent { target, amount: 50 });
    /// }
    /// ```
    pub fn stream_mut<E: Event>(&mut self) -> Option<&mut Stream<E>> {
        let stream = self.streams.get_mut(&TypeId::of::<E>())?;
        stream.as_any_mut().downcast_mut::<Stream<E>>()
    }

    /// Swaps all event stream buffers.
    ///
    /// This should be called once per frame, typically at the start of the frame
    /// before any systems run. After swap:
    ///
    /// - Events sent last frame become readable via [`stream()`](Self::stream)
    /// - Events from the previous frame are cleared
    /// - New events can be sent to the fresh active buffer
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Game loop
    /// loop {
    ///     // Start of frame: swap buffers
    ///     world.events_mut().swap_all();
    ///
    ///     // Run systems - they read last frame's events, write this frame's
    ///     schedule.run(&mut world);
    /// }
    /// ```
    pub fn swap_all(&mut self) {
        for stream in self.streams.values_mut() {
            stream.swap();
        }
    }
}

impl Default for Broker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TestEvent {
        value: u32,
    }
    impl Event for TestEvent {}

    #[derive(Clone, Debug)]
    struct OtherEvent;
    impl Event for OtherEvent {}

    // ==================== Registration ====================

    #[test]
    fn new_creates_empty_broker() {
        let broker = Broker::new();
        assert!(!broker.is_registered::<TestEvent>());
    }

    #[test]
    fn register_adds_stream() {
        let mut broker = Broker::new();

        broker.register::<TestEvent>();

        assert!(broker.is_registered::<TestEvent>());
    }

    #[test]
    fn register_with_capacity_adds_stream() {
        let mut broker = Broker::new();

        broker.register_with_capacity::<TestEvent>(512);

        assert!(broker.is_registered::<TestEvent>());
    }

    #[test]
    #[should_panic(expected = "Event type already registered")]
    fn register_duplicate_panics() {
        let mut broker = Broker::new();
        broker.register::<TestEvent>();

        broker.register::<TestEvent>(); // Should panic
    }

    #[test]
    fn register_multiple_types() {
        let mut broker = Broker::new();

        broker.register::<TestEvent>();
        broker.register::<OtherEvent>();

        assert!(broker.is_registered::<TestEvent>());
        assert!(broker.is_registered::<OtherEvent>());
    }

    // ==================== Stream Access ====================

    #[test]
    fn stream_returns_none_for_unregistered() {
        let broker = Broker::new();

        assert!(broker.stream::<TestEvent>().is_none());
    }

    #[test]
    fn stream_returns_registered_stream() {
        let mut broker = Broker::new();
        broker.register::<TestEvent>();

        assert!(broker.stream::<TestEvent>().is_some());
    }

    #[test]
    fn stream_mut_returns_none_for_unregistered() {
        let mut broker = Broker::new();

        assert!(broker.stream_mut::<TestEvent>().is_none());
    }

    #[test]
    fn stream_mut_allows_sending_events() {
        let mut broker = Broker::new();
        broker.register::<TestEvent>();

        let stream = broker.stream_mut::<TestEvent>().unwrap();
        stream.send(TestEvent { value: 42 });

        // Events in active buffer, not visible in stable yet
        assert!(broker.stream::<TestEvent>().unwrap().is_empty());
    }

    // ==================== Swap ====================

    #[test]
    fn swap_all_makes_events_readable() {
        let mut broker = Broker::new();
        broker.register::<TestEvent>();

        // Send event
        broker
            .stream_mut::<TestEvent>()
            .unwrap()
            .send(TestEvent { value: 42 });

        // Before swap: stable is empty
        assert!(broker.stream::<TestEvent>().unwrap().is_empty());

        // Swap
        broker.swap_all();

        // After swap: event is readable
        let stream = broker.stream::<TestEvent>().unwrap();
        assert_eq!(stream.len(), 1);
        assert_eq!(stream.iter().next(), Some(&TestEvent { value: 42 }));
    }

    #[test]
    fn swap_all_clears_old_stable() {
        let mut broker = Broker::new();
        broker.register::<TestEvent>();

        // Send and swap
        broker
            .stream_mut::<TestEvent>()
            .unwrap()
            .send(TestEvent { value: 1 });
        broker.swap_all();

        // Event readable
        assert_eq!(broker.stream::<TestEvent>().unwrap().len(), 1);

        // Swap again without sending
        broker.swap_all();

        // Old events cleared
        assert!(broker.stream::<TestEvent>().unwrap().is_empty());
    }

    #[test]
    fn swap_all_swaps_multiple_streams() {
        let mut broker = Broker::new();
        broker.register::<TestEvent>();
        broker.register::<OtherEvent>();

        broker
            .stream_mut::<TestEvent>()
            .unwrap()
            .send(TestEvent { value: 1 });
        broker.stream_mut::<OtherEvent>().unwrap().send(OtherEvent);

        broker.swap_all();

        assert_eq!(broker.stream::<TestEvent>().unwrap().len(), 1);
        assert_eq!(broker.stream::<OtherEvent>().unwrap().len(), 1);
    }
}
