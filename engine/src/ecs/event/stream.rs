//! Double-buffered event stream storage.
//!
//! This module provides [`Stream`], the core storage type for the event system.
//! Each event type gets its own stream with two internal buffers that swap each frame.
//!
//! # Double-Buffer Model
//!
//! The stream maintains two buffers:
//! - **Active buffer**: Where new events are written via [`send()`](Stream::send)
//! - **Stable buffer**: Where events are read via [`iter()`](Stream::iter)
//!
//! When [`swap()`](Stream::swap) is called (typically once per frame):
//! 1. The active buffer becomes the stable buffer (events become readable)
//! 2. The old stable buffer is cleared and becomes the new active buffer
//!
//! This design allows writers and readers to operate on different buffers,
//! enabling parallel access without conflicts.
//!
//! # Example
//!
//! ```rust,ignore
//! let mut stream = Stream::<MyEvent>::new(1024);
//!
//! // Frame N: Write events
//! stream.send(MyEvent { value: 1 });
//! stream.send(MyEvent { value: 2 });
//!
//! // Events not yet readable (in active buffer)
//! assert!(stream.is_empty());
//!
//! // End of frame: swap buffers
//! stream.swap();
//!
//! // Frame N+1: Events now readable
//! for event in stream.iter() {
//!     println!("{:?}", event);
//! }
//! ```

use std::any::Any;

use crate::ecs::event::Event;

/// Double-buffered storage for a single event type.
///
/// `Stream<E>` stores events of type `E` using a double-buffer pattern.
/// Writers append to the active buffer, while readers iterate over the stable buffer.
/// Buffers are swapped by the [`Broker`](super::Broker) at frame boundaries.
///
/// # Capacity
///
/// Each stream has a fixed capacity set at creation. Attempting to send more events
/// than the capacity allows will panic. Choose capacity based on expected event volume:
/// - Input events: 64-256 (bounded by input rate)
/// - Gameplay events: 1024+ (collisions, damage, etc.)
/// - High-frequency events: 4096+ (particles, audio triggers)
///
/// # Thread Safety
///
/// `Stream` itself is not thread-safe. Thread safety is provided by the
/// scheduler's access control system, which ensures that:
/// - Only one system can write to a stream at a time (exclusive access)
/// - Multiple systems can read from a stream simultaneously (shared access)
/// - Writers and readers access different buffers (no conflicts)
pub struct Stream<E: Event> {
    /// Index of the currently active (write) buffer: 0 or 1
    active_index: usize,

    /// The two buffers - one active, one stable
    buffers: [Vec<E>; 2],

    /// Capacity limit - panic on overflow
    capacity: usize,
}

impl<E: Event> Stream<E> {
    /// Creates a new event stream with the specified capacity.
    ///
    /// Both internal buffers are pre-allocated to the given capacity to avoid
    /// allocations during gameplay.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of events that can be sent per frame.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create a stream that can hold up to 1024 events per frame
    /// let stream = Stream::<DamageEvent>::new(1024);
    /// ```
    pub fn new(capacity: usize) -> Self {
        Self {
            active_index: 0,
            buffers: [Vec::with_capacity(capacity), Vec::with_capacity(capacity)],
            capacity,
        }
    }

    /// Send an event to the active buffer.
    ///
    /// # Panics
    ///
    /// Panics if the active buffer would exceed capacity.
    pub fn send(&mut self, event: E) {
        assert!(
            self.buffers[self.active_index].len() < self.capacity,
            "Event stream capacity exceeded: {} events (capacity: {})",
            self.buffers[self.active_index].len(),
            self.capacity
        );
        self.buffers[self.active_index].push(event);
    }

    /// Returns an iterator over events in the stable buffer.
    ///
    /// Events are yielded in the order they were sent. This only includes events
    /// sent before the last [`swap()`](Self::swap) call.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for event in stream.iter() {
    ///     println!("Processing: {:?}", event);
    /// }
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.stable_buffer().iter()
    }

    /// Returns the number of events in the stable buffer.
    ///
    /// This count reflects events available for reading, not events currently
    /// being written to the active buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.stable_buffer().len()
    }

    /// Returns `true` if the stable buffer contains no events.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.stable_buffer().is_empty()
    }

    /// Swaps the active and stable buffers.
    ///
    /// After swap:
    /// - Events in the old active buffer become readable via [`iter()`](Self::iter)
    /// - The old stable buffer is cleared and becomes the new active buffer
    ///
    /// This is called by [`Broker::swap_all()`](super::Broker::swap_all)
    /// and should not be called directly.
    pub(crate) fn swap(&mut self) {
        self.active_index = 1 - self.active_index;
        self.buffers[self.active_index].clear();
    }

    /// Returns a reference to the stable (readable) buffer.
    #[inline]
    fn stable_buffer(&self) -> &Vec<E> {
        &self.buffers[1 - self.active_index]
    }
}

/// Type-erased interface for event streams.
///
/// This trait enables [`Broker`](super::Broker) to store heterogeneous
/// `Stream<E>` instances in a single collection while still supporting
/// operations that don't require knowing the concrete event type.
///
/// # Type Erasure Pattern
///
/// The broker stores `Box<dyn ErasedStream>` and uses [`as_any()`](Self::as_any)
/// and [`as_any_mut()`](Self::as_any_mut) to downcast back to the concrete
/// `Stream<E>` when type-specific access is needed.
pub(crate) trait ErasedStream: Send + Sync {
    /// Swaps the active and stable buffers.
    fn swap(&mut self);

    /// Returns the number of events in the stable buffer.
    fn stable_len(&self) -> usize;

    /// Returns a reference to self as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to self as `&mut dyn Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<E: Event> ErasedStream for Stream<E> {
    fn swap(&mut self) {
        Stream::swap(self);
    }

    fn stable_len(&self) -> usize {
        self.len()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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

    // ==================== Basic Operations ====================

    #[test]
    fn new_creates_empty_stream() {
        let stream = Stream::<TestEvent>::new(100);

        assert!(stream.is_empty());
        assert_eq!(stream.len(), 0);
    }

    #[test]
    fn send_adds_to_active_buffer() {
        let mut stream = Stream::<TestEvent>::new(100);

        stream.send(TestEvent { value: 42 });

        // Stable buffer still empty (event is in active)
        assert!(stream.is_empty());
    }

    #[test]
    fn iter_returns_stable_buffer_events() {
        let mut stream = Stream::<TestEvent>::new(100);

        stream.send(TestEvent { value: 1 });
        stream.swap();

        let events: Vec<_> = stream.iter().collect();
        assert_eq!(events, vec![&TestEvent { value: 1 }]);
    }

    #[test]
    fn len_returns_stable_buffer_count() {
        let mut stream = Stream::<TestEvent>::new(100);

        stream.send(TestEvent { value: 1 });
        stream.send(TestEvent { value: 2 });
        assert_eq!(stream.len(), 0); // Still in active

        stream.swap();
        assert_eq!(stream.len(), 2); // Now in stable
    }

    // ==================== Swap Behavior ====================

    #[test]
    fn swap_moves_active_to_stable() {
        let mut stream = Stream::<TestEvent>::new(100);

        stream.send(TestEvent { value: 42 });
        assert!(stream.is_empty()); // Before swap

        stream.swap();

        assert!(!stream.is_empty()); // After swap
        assert_eq!(stream.iter().next(), Some(&TestEvent { value: 42 }));
    }

    #[test]
    fn swap_clears_new_active() {
        let mut stream = Stream::<TestEvent>::new(100);

        stream.send(TestEvent { value: 1 });
        stream.swap();

        // Send more events to new active
        stream.send(TestEvent { value: 2 });
        stream.swap();

        // Should only see the new event, old one cleared
        let events: Vec<_> = stream.iter().collect();
        assert_eq!(events, vec![&TestEvent { value: 2 }]);
    }

    #[test]
    fn multiple_swaps_cycle_correctly() {
        let mut stream = Stream::<TestEvent>::new(100);

        // Frame 1: send event
        stream.send(TestEvent { value: 1 });
        stream.swap();
        assert_eq!(stream.len(), 1);

        // Frame 2: send different event
        stream.send(TestEvent { value: 2 });
        stream.swap();
        assert_eq!(stream.len(), 1);
        assert_eq!(stream.iter().next(), Some(&TestEvent { value: 2 }));

        // Frame 3: no events
        stream.swap();
        assert!(stream.is_empty());
    }

    // ==================== Capacity ====================

    #[test]
    fn send_up_to_capacity_succeeds() {
        let mut stream = Stream::<TestEvent>::new(3);

        stream.send(TestEvent { value: 1 });
        stream.send(TestEvent { value: 2 });
        stream.send(TestEvent { value: 3 });

        // Should not panic
        stream.swap();
        assert_eq!(stream.len(), 3);
    }

    #[test]
    #[should_panic(expected = "Event stream capacity exceeded")]
    fn send_over_capacity_panics() {
        let mut stream = Stream::<TestEvent>::new(2);

        stream.send(TestEvent { value: 1 });
        stream.send(TestEvent { value: 2 });
        stream.send(TestEvent { value: 3 }); // Should panic
    }

    // ==================== Multiple Events ====================

    #[test]
    fn preserves_event_order() {
        let mut stream = Stream::<TestEvent>::new(100);

        stream.send(TestEvent { value: 1 });
        stream.send(TestEvent { value: 2 });
        stream.send(TestEvent { value: 3 });
        stream.swap();

        let values: Vec<_> = stream.iter().map(|e| e.value).collect();
        assert_eq!(values, vec![1, 2, 3]);
    }

    // ==================== ErasedStream Trait ====================

    #[test]
    fn erased_stream_swap_works() {
        let mut stream = Stream::<TestEvent>::new(100);
        stream.send(TestEvent { value: 42 });

        let erased: &mut dyn ErasedStream = &mut stream;
        erased.swap();

        assert_eq!(stream.len(), 1);
    }

    #[test]
    fn erased_stream_stable_len_works() {
        let mut stream = Stream::<TestEvent>::new(100);
        stream.send(TestEvent { value: 1 });
        stream.send(TestEvent { value: 2 });
        stream.swap();

        let erased: &dyn ErasedStream = &stream;
        assert_eq!(erased.stable_len(), 2);
    }

    #[test]
    fn erased_stream_downcast_works() {
        let mut stream = Stream::<TestEvent>::new(100);
        stream.send(TestEvent { value: 42 });
        stream.swap();

        let erased: &dyn ErasedStream = &stream;
        let downcasted = erased.as_any().downcast_ref::<Stream<TestEvent>>();

        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().len(), 1);
    }
}
