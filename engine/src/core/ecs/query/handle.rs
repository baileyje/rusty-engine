//! Callback-based query API that eliminates lifetimes from system function signatures.
//!
//! This module provides [`QueryHandle`], which allows querying entities via callbacks
//! instead of iterators. The key benefit is that system function signatures don't need
//! explicit lifetime parameters - lifetimes are scoped to the callback invocation.

use std::marker::PhantomData;

use crate::core::ecs::{entity, query::data::DataSpec, query::Data, storage, world};

/// A handle to execute queries via callbacks.
///
/// Unlike [`Query`](super::Query) which returns iterators that expose lifetimes,
/// `QueryHandle` provides callback methods where lifetimes are scoped to the callback
/// invocation. This allows system signatures to be clean and lifetime-free.
///
/// # Type Parameters
///
/// - `'w`: The lifetime of the world being queried
/// - `D`: The query data specification (e.g., `&Component`, `(&C1, &mut C2)`)
///
/// # Examples
///
/// ```rust,ignore
/// use rusty_engine::core::ecs::query::QueryHandle;
/// use rusty_macros::Component;
///
/// #[derive(Component)]
/// struct Position { x: f32, y: f32 }
///
/// #[derive(Component)]
/// struct Velocity { dx: f32, dy: f32 }
///
/// // Clean function signature - no lifetime parameters!
/// fn movement_system(query: QueryHandle<(&Velocity, &mut Position)>) {
///     query.for_each_mut(|(vel, pos)| {
///         pos.x += vel.dx;
///         pos.y += vel.dy;
///     });
/// }
/// ```
pub struct QueryHandle<'w, D> {
    /// Reference to the world (still borrows, but lifetime not exposed to user).
    world: &'w mut world::World,

    /// Specification of what components to query.
    spec: DataSpec,

    /// Table IDs that match this query.
    table_ids: Vec<storage::table::Id>,

    /// Phantom data for the query type.
    _marker: PhantomData<D>,
}

impl<'w, D> QueryHandle<'w, D> {
    /// Create a new query handle.
    ///
    /// This is typically called by the system parameter machinery, not directly by users.
    ///
    /// # Parameters
    ///
    /// - `world`: Mutable reference to the world to query
    /// - `spec`: The data specification describing what to query
    pub(crate) fn new(world: &'w mut world::World, spec: DataSpec) -> Self {
        // Find matching tables
        let table_ids = {
            let storage = world.storage();
            storage.supporting(&spec.as_component_spec())
        };

        Self {
            world,
            spec,
            table_ids,
            _marker: PhantomData,
        }
    }

    /// Get the number of entities that match this query.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn check_enemies(enemies: QueryHandle<&Enemy>) {
    ///     println!("Enemy count: {}", enemies.count());
    /// }
    /// ```
    pub fn count(&self) -> usize
    where
        D: Data<'w>,
    {
        let mut count = 0;
        unsafe {
            let storage: &storage::Storage = &*(self.world.storage() as *const _);
            for &table_id in &self.table_ids {
                let table = storage.get(table_id);
                count += table.len();
            }
        }
        count
    }

    /// Check if the query has any matches.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn check_game_over(players: QueryHandle<&Player>) {
    ///     if players.is_empty() {
    ///         println!("Game Over!");
    ///     }
    /// }
    /// ```
    pub fn is_empty(&self) -> bool
    where
        D: Data<'w>,
    {
        self.count() == 0
    }

    /// Execute a callback for each entity matching the query (immutable access).
    ///
    /// The callback receives data matching the query specification. The lifetime
    /// of the data is scoped to the callback invocation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn print_positions(query: QueryHandle<&Position>) {
    ///     query.for_each(|pos| {
    ///         println!("Position: ({}, {})", pos.x, pos.y);
    ///     });
    /// }
    /// ```
    pub fn for_each<F>(&self, mut f: F)
    where
        D: Data<'w>,
        F: FnMut(<D as Data<'w>>::ReadOnly),
    {
        unsafe {
            // Cast to 'w lifetime for Data::fetch
            let storage: &'w storage::Storage = &*(self.world.storage() as *const _);

            for &table_id in &self.table_ids {
                let table: &'w storage::Table = storage.get(table_id);

                for row_idx in 0..table.len() {
                    let row = storage::Row::new(row_idx);

                    if let Some(entity_ref) = table.entity(row) {
                        let entity = entity::Entity::from(entity_ref);

                        // Fetch immutable data
                        if let Some(data) = D::fetch(entity, table, row) {
                            f(data);
                        }
                    }
                }
            }
        }
    }

    /// Execute a callback for each entity matching the query (mutable access).
    ///
    /// The callback receives mutable data matching the query specification. The lifetime
    /// of the data is scoped to the callback invocation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn apply_gravity(query: QueryHandle<&mut Velocity>) {
    ///     query.for_each_mut(|vel| {
    ///         vel.dy -= 9.8;
    ///     });
    /// }
    /// ```
    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        D: Data<'w>,
        F: FnMut(D),
    {
        unsafe {
            // Get raw pointer to storage to avoid aliasing issues
            let storage_ptr = self.world.storage_mut() as *mut storage::Storage;

            for &table_id in &self.table_ids {
                let storage = &mut *storage_ptr;
                let table_ptr = storage.get_mut(table_id) as *mut storage::Table;
                let table = &*table_ptr;

                for row_idx in 0..table.len() {
                    let row = storage::Row::new(row_idx);

                    if let Some(entity_ref) = table.entity(row) {
                        let entity = entity::Entity::from(entity_ref);

                        // Create mutable reference with 'w lifetime for fetch_mut
                        let table_mut: &'w mut storage::Table = &mut *table_ptr;

                        // Use aliasing pattern for multiple component access
                        if let Some(data) = D::fetch_mut(
                            entity,
                            &mut *(table_mut as *mut storage::Table),
                            row,
                        ) {
                            f(data);
                        }
                    }
                }
            }
        }
    }

    /// Find the first entity matching a predicate.
    ///
    /// Returns `true` if any entity matches the predicate, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn check_winner(players: QueryHandle<&Player>) -> bool {
    ///     players.find(|player| player.score > 1000)
    /// }
    /// ```
    pub fn find<F>(&self, mut predicate: F) -> bool
    where
        D: Data<'w>,
        F: FnMut(&<D as Data<'w>>::ReadOnly) -> bool,
    {
        unsafe {
            // Cast to 'w lifetime for Data::fetch
            let storage: &'w storage::Storage = &*(self.world.storage() as *const _);

            for &table_id in &self.table_ids {
                let table: &'w storage::Table = storage.get(table_id);

                for row_idx in 0..table.len() {
                    let row = storage::Row::new(row_idx);

                    if let Some(entity_ref) = table.entity(row) {
                        let entity = entity::Entity::from(entity_ref);

                        if let Some(data) = D::fetch(entity, table, row) {
                            if predicate(&data) {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        false
    }

    /// Check if any entity matches a predicate.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn has_high_scorer(players: QueryHandle<&Player>) -> bool {
    ///     players.any(|player| player.score > 1000)
    /// }
    /// ```
    pub fn any<F>(&self, predicate: F) -> bool
    where
        D: Data<'w>,
        F: FnMut(&<D as Data<'w>>::ReadOnly) -> bool,
    {
        self.find(predicate)
    }

    /// Check if all entities match a predicate.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn all_ready(players: QueryHandle<&Player>) -> bool {
    ///     players.all(|player| player.is_ready)
    /// }
    /// ```
    pub fn all<F>(&self, mut predicate: F) -> bool
    where
        D: Data<'w>,
        F: FnMut(&<D as Data<'w>>::ReadOnly) -> bool,
    {
        !self.find(|data| !predicate(data))
    }
}
