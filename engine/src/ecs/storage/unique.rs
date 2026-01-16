//! Type-erased storage for singleton (unique) values.
//!
//! This module provides [`Uniques`], a container for storing singleton values that are
//! accessible throughout the ECS world. Unlike components which are attached to entities,
//! uniques are global values that exist independently.
//!
//! # Overview
//!
//! Uniques are useful for:
//! - Global game state (score, time, settings)
//! - Shared services (asset loaders, input handlers)
//! - Configuration data
//! - Any singleton data that doesn't belong to a specific entity
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
//! // Insert uniques
//! let mut uniques = Uniques::new();
//! uniques.insert(GameTime { elapsed: 0.0, delta: 0.016 });
//! uniques.insert(Score(0));
//!
//! // Access uniques
//! if let Some(time) = uniques.get::<GameTime>() {
//!     println!("Elapsed: {}", time.elapsed);
//! }
//!
//! // Mutate uniques
//! if let Some(score) = uniques.get_mut::<Score>() {
//!     score.0 += 100;
//! }
//!
//! // Check existence
//! if uniques.contains::<Score>() {
//!     println!("Score unique exists");
//! }
//!
//! // Remove uniques
//! let old_score = uniques.remove::<Score>();
//! ```
//!
//! # Thread Safety
//!
//! The `Uniques` container itself is not thread-safe. External synchronization is required
//! for concurrent access. The scheduler's access control system handles this by tracking
//! unique access requirements and preventing conflicting concurrent access.

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use crate::ecs::unique;

/// Type-erased storage for singleton (unique) values.
///
/// `Uniques` stores values by their [`TypeId`], allowing heterogeneous storage of any type
/// implementing the [`Unique`](unique::Unique) trait. Each unique type can have at most
/// one instance stored.
///
/// # Storage Model
///
/// Uniques are stored using `TypeId` as the key rather than a numeric ID. This avoids
/// requiring a registry lookup for every access operation, trading some memory overhead
/// for faster runtime access.
///
/// # Type Safety
///
/// Despite using type erasure internally (`Box<dyn Any>`), all public methods are fully
/// type-safe through generic parameters. The `Unique` trait bound ensures only valid
/// unique types can be stored.
pub struct Uniques {
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Uniques {
    /// Creates a new, empty unique storage.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let uniques = Uniques::new();
    /// assert!(!uniques.contains::<MyUnique>());
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    /// Inserts a unique into storage.
    ///
    /// If a unique of the same type already exists, it will be replaced and the old
    /// value will be dropped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// uniques.insert(Score(100));
    ///
    /// // Replacing overwrites the previous value
    /// uniques.insert(Score(200));
    /// assert_eq!(uniques.get::<Score>().unwrap().0, 200);
    /// ```
    #[inline]
    pub fn insert<U: unique::Unique>(&mut self, value: U) {
        self.data.insert(TypeId::of::<U>(), Box::new(value));
    }

    /// Returns a reference to a unique, if it exists.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// uniques.insert(Score(100));
    ///
    /// if let Some(score) = uniques.get::<Score>() {
    ///     println!("Current score: {}", score.0);
    /// }
    /// ```
    #[inline]
    pub fn get<U: unique::Unique>(&self) -> Option<&U> {
        self.data
            .get(&TypeId::of::<U>())
            .and_then(|stored| stored.downcast_ref::<U>())
    }

    /// Returns a mutable reference to a unique, if it exists.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// uniques.insert(Score(100));
    ///
    /// if let Some(score) = uniques.get_mut::<Score>() {
    ///     score.0 += 50;
    /// }
    /// ```
    #[inline]
    pub fn get_mut<U: unique::Unique>(&mut self) -> Option<&mut U> {
        self.data
            .get_mut(&TypeId::of::<U>())
            .and_then(|stored| stored.downcast_mut::<U>())
    }

    /// Removes a unique from storage, returning it if it existed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// uniques.insert(Score(100));
    ///
    /// let removed = uniques.remove::<Score>();
    /// assert_eq!(removed.unwrap().0, 100);
    /// assert!(!uniques.contains::<Score>());
    /// ```
    #[inline]
    pub fn remove<U: unique::Unique>(&mut self) -> Option<U> {
        self.data
            .remove(&TypeId::of::<U>())
            .and_then(|stored| (stored as Box<dyn Any>).downcast::<U>().ok())
            .map(|boxed| *boxed)
    }

    /// Returns `true` if a unique of type `R` exists in storage.
    ///
    /// This is useful for checking existence without borrowing the unique.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// assert!(!uniques.contains::<Score>());
    ///
    /// uniques.insert(Score(0));
    /// assert!(uniques.contains::<Score>());
    /// ```
    #[inline]
    pub fn contains<U: unique::Unique>(&self) -> bool {
        self.data.contains_key(&TypeId::of::<U>())
    }

    /// Returns the number of uniques currently stored.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// assert_eq!(uniques.len(), 0);
    ///
    /// uniques.insert(Score(0));
    /// uniques.insert(GameTime { elapsed: 0.0 });
    /// assert_eq!(uniques.len(), 2);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if no uniques are stored.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// assert!(uniques.is_empty());
    ///
    /// uniques.insert(Score(0));
    /// assert!(!uniques.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Removes all uniques from storage.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut uniques = Uniques::new();
    /// uniques.insert(Score(0));
    /// uniques.insert(GameTime { elapsed: 0.0 });
    ///
    /// uniques.clear();
    /// assert!(uniques.is_empty());
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

impl Default for Uniques {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Unique;

    use super::*;

    // Test resource types
    #[derive(Unique, Debug, PartialEq)]
    struct ScoreBoard(u32);

    #[derive(Unique, Debug, PartialEq)]
    struct GameTime {
        elapsed: f32,
        delta: f32,
    }

    #[derive(Unique, Debug, PartialEq)]
    struct PlayerName(String);

    // ==================== Basic Operations ====================

    #[test]
    fn new_creates_empty_storage() {
        let resources = Uniques::new();

        assert!(resources.is_empty());
        assert_eq!(resources.len(), 0);
    }

    #[test]
    fn default_creates_empty_storage() {
        let resources = Uniques::default();

        assert!(resources.is_empty());
        assert_eq!(resources.len(), 0);
    }

    #[test]
    fn insert_stores_resource() {
        let mut resources = Uniques::new();

        resources.insert(ScoreBoard(100));

        assert!(resources.contains::<ScoreBoard>());
        assert_eq!(resources.len(), 1);
    }

    #[test]
    fn insert_replaces_existing_resource() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(100));

        resources.insert(ScoreBoard(200));

        assert_eq!(resources.get::<ScoreBoard>().unwrap().0, 200);
        assert_eq!(resources.len(), 1); // Still only one resource
    }

    // ==================== Get Operations ====================

    #[test]
    fn get_returns_none_for_missing_resource() {
        let resources = Uniques::new();

        assert!(resources.get::<ScoreBoard>().is_none());
    }

    #[test]
    fn get_returns_reference_to_stored_resource() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(250));

        let score = resources.get::<ScoreBoard>().unwrap();

        assert_eq!(score.0, 250);
    }

    #[test]
    fn get_mut_returns_none_for_missing_resource() {
        let mut resources = Uniques::new();

        assert!(resources.get_mut::<ScoreBoard>().is_none());
    }

    #[test]
    fn get_mut_allows_modification() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(250));

        resources.get_mut::<ScoreBoard>().unwrap().0 += 100;

        assert_eq!(resources.get::<ScoreBoard>().unwrap().0, 350);
    }

    // ==================== Remove Operations ====================

    #[test]
    fn remove_returns_none_for_missing_resource() {
        let mut resources = Uniques::new();

        assert!(resources.remove::<ScoreBoard>().is_none());
    }

    #[test]
    fn remove_returns_and_removes_resource() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(250));

        let removed = resources.remove::<ScoreBoard>().unwrap();

        assert_eq!(removed.0, 250);
        assert!(!resources.contains::<ScoreBoard>());
        assert!(resources.is_empty());
    }

    // ==================== Contains Operations ====================

    #[test]
    fn contains_returns_false_for_missing_resource() {
        let resources = Uniques::new();

        assert!(!resources.contains::<ScoreBoard>());
    }

    #[test]
    fn contains_returns_true_for_stored_resource() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(0));

        assert!(resources.contains::<ScoreBoard>());
    }

    #[test]
    fn contains_returns_false_after_removal() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(0));
        resources.remove::<ScoreBoard>();

        assert!(!resources.contains::<ScoreBoard>());
    }

    // ==================== Multiple Resource Types ====================

    #[test]
    fn stores_multiple_resource_types() {
        let mut resources = Uniques::new();

        resources.insert(ScoreBoard(100));
        resources.insert(GameTime {
            elapsed: 1.5,
            delta: 0.016,
        });
        resources.insert(PlayerName("Alice".to_string()));

        assert_eq!(resources.len(), 3);
        assert!(resources.contains::<ScoreBoard>());
        assert!(resources.contains::<GameTime>());
        assert!(resources.contains::<PlayerName>());
    }

    #[test]
    fn resource_types_are_independent() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(100));
        resources.insert(GameTime {
            elapsed: 1.5,
            delta: 0.016,
        });

        // Removing one doesn't affect the other
        resources.remove::<ScoreBoard>();

        assert!(!resources.contains::<ScoreBoard>());
        assert!(resources.contains::<GameTime>());
        assert_eq!(resources.len(), 1);
    }

    #[test]
    fn get_correct_type_with_multiple_resources() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(42));
        resources.insert(GameTime {
            elapsed: 3.34,
            delta: 0.016,
        });

        assert_eq!(resources.get::<ScoreBoard>().unwrap().0, 42);
        assert_eq!(resources.get::<GameTime>().unwrap().elapsed, 3.34);
    }

    // ==================== Len and IsEmpty ====================

    #[test]
    fn len_tracks_resource_count() {
        let mut resources = Uniques::new();

        assert_eq!(resources.len(), 0);

        resources.insert(ScoreBoard(0));
        assert_eq!(resources.len(), 1);

        resources.insert(GameTime {
            elapsed: 0.0,
            delta: 0.0,
        });
        assert_eq!(resources.len(), 2);

        resources.remove::<ScoreBoard>();
        assert_eq!(resources.len(), 1);
    }

    #[test]
    fn is_empty_reflects_storage_state() {
        let mut resources = Uniques::new();

        assert!(resources.is_empty());

        resources.insert(ScoreBoard(0));
        assert!(!resources.is_empty());

        resources.remove::<ScoreBoard>();
        assert!(resources.is_empty());
    }

    // ==================== Clear ====================

    #[test]
    fn clear_removes_all_resources() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(100));
        resources.insert(GameTime {
            elapsed: 1.0,
            delta: 0.016,
        });
        resources.insert(PlayerName("Bob".to_string()));

        resources.clear();

        assert!(resources.is_empty());
        assert_eq!(resources.len(), 0);
        assert!(!resources.contains::<ScoreBoard>());
        assert!(!resources.contains::<GameTime>());
        assert!(!resources.contains::<PlayerName>());
    }

    #[test]
    fn clear_on_empty_is_safe() {
        let mut resources = Uniques::new();

        resources.clear(); // Should not panic

        assert!(resources.is_empty());
    }

    // ==================== Complex Resource Types ====================

    #[test]
    fn stores_resource_with_string_field() {
        let mut resources = Uniques::new();

        resources.insert(PlayerName("Charlie".to_string()));

        let name = resources.get::<PlayerName>().unwrap();
        assert_eq!(name.0, "Charlie");
    }

    #[test]
    fn mutates_resource_with_struct_fields() {
        let mut resources = Uniques::new();
        resources.insert(GameTime {
            elapsed: 0.0,
            delta: 0.016,
        });

        {
            let time = resources.get_mut::<GameTime>().unwrap();
            time.elapsed += time.delta;
        }

        assert_eq!(resources.get::<GameTime>().unwrap().elapsed, 0.016);
    }

    // ==================== Edge Cases ====================

    #[test]
    fn insert_after_remove_works() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(100));
        resources.remove::<ScoreBoard>();

        resources.insert(ScoreBoard(200));

        assert_eq!(resources.get::<ScoreBoard>().unwrap().0, 200);
    }

    #[test]
    fn multiple_removes_are_safe() {
        let mut resources = Uniques::new();
        resources.insert(ScoreBoard(100));

        let first = resources.remove::<ScoreBoard>();
        let second = resources.remove::<ScoreBoard>();

        assert!(first.is_some());
        assert!(second.is_none());
    }
}
