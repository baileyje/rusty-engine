//! Query result iterator that yields matching entities across multiple tables.
//!
//! This module provides the [`Result`] iterator, which is returned by [`Query::invoke`]
//! and iterates over all entities matching the query's component requirements.
//!
//! # Architecture
//!
//! The result iterator traverses multiple tables (archetypes) sequentially:
//!
//! 1. For each table that contains all required components
//! 2. For each entity row within that table
//! 3. Fetch the requested component data
//!
//! # Features
//!
//! - **Exact size**: Implements [`ExactSizeIterator`] for efficient allocation
//! - **Multi-table**: Seamlessly iterates across archetype boundaries
//! - **Zero-copy**: Returns references directly into storage columns
//!
//! # Example
//!
//! ```rust,ignore
//! let query = Query::<(&Position, &mut Velocity)>::new(world.components());
//! let results = query.invoke(&mut world);
//!
//! // Know the count ahead of time
//! println!("Found {} entities", results.len());
//!
//! // Iterate over all matches
//! for (pos, vel) in results {
//!     vel.dx += pos.x * 0.01;
//! }
//! ```
//!
//! [`Query::invoke`]: super::Query::invoke

use std::marker::PhantomData;

use crate::ecs::{
    query::{data::Data, source::DataSource},
    storage,
};

/// An iterator over query results that yields entities matching the query specification.
///
/// This iterator is returned by [`Query::invoke`] and implements both [`Iterator`]
/// and [`ExactSizeIterator`], allowing you to iterate over matching entities
/// and know the exact count ahead of time.
///
/// # Design
///
/// The iterator maintains internal state to traverse multiple tables (archetypes)
/// sequentially. It:
/// - Iterates through tables that contain all required components
/// - Within each table, iterates through rows (entities)
/// - Fetches the requested component data for each entity
///
/// # Type Parameters
///
/// - `'w`: The lifetime of the world being queried
/// - `D`: The query data type (implements [`Data`])
///
/// # Examples
///
/// ```rust,ignore
/// let query = Query::<(&Position, &mut Velocity)>::new(world.components());
/// let mut results = query.invoke(&mut world);
///
/// // Iterate over results
/// for (pos, vel) in results {
///     vel.dx += pos.x * 0.01;
/// }
///
/// // Can also check the count
/// let count = results.len();
/// ```
///
/// [`Query::invoke`]: super::Query::invoke
/// [`Data`]: super::data::Data
pub struct Result<'w, D: Data> {
    /// Mutable reference to the data source being queried.
    source: &'w mut dyn DataSource,

    /// List of table IDs that match the query specification.
    table_ids: Vec<storage::TableId>,

    /// Current table index in the table_ids vector.
    table_index: usize,

    /// Current row index within the current table.
    row_index: usize,

    /// Total number of items yielded so far.
    index: usize,

    /// Total number of items that will be yielded (pre-calculated).
    len: usize,

    /// Phantom data to tie the query type to the struct.
    _marker: PhantomData<D>,
}

impl<'w, D: Data> Result<'w, D> {
    /// Construct a new query result iterator.
    ///
    /// This is called internally by [`Query::invoke`] and pre-calculates the total
    /// number of entities that will be yielded by iterating through all matching tables.
    ///
    /// # Parameters
    ///
    /// - `world`: Mutable reference to the world being queried
    /// - `table_ids`: List of table IDs that contain all required components
    ///
    /// # Performance
    ///
    /// The constructor iterates through all tables once to calculate the total length,
    /// enabling [`ExactSizeIterator`] support.
    ///
    /// [`Query::invoke`]: super::Query::invoke
    #[inline]
    pub fn new(source: &'w mut dyn DataSource, table_ids: Vec<storage::TableId>) -> Self {
        // Pre-calculate the total length for ExactSizeIterator support.
        let mut len = 0;
        for table_id in table_ids.iter() {
            // Safety: We know this is a valid table as we got this ID from the registry
            // before creating this result.
            len += source.table(*table_id).entities().len();
        }

        Self {
            source,
            table_ids,
            table_index: 0,
            row_index: 0,
            len,
            index: 0,
            _marker: PhantomData,
        }
    }

    /// Get the length of the query results.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Determine if the query results are empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<'w, D: Data> Iterator for Result<'w, D> {
    type Item = D::Data<'w>;

    /// Advance the iterator and return the next query result.
    ///
    /// This method:
    /// 1. Checks if there are more items to yield
    /// 2. Fetches the current table and row
    /// 3. Retrieves the entity at that row
    /// 4. Advances the internal state (row/table indices)
    /// 5. Fetches the requested component data using unsafe aliasing
    ///
    /// # Safety
    ///
    /// This method uses raw pointer aliasing to create multiple mutable references
    /// to the same table. This is safe because:
    /// - The query system validates at invocation time that no component is
    ///   requested multiple times
    /// - Each fetch_mut call accesses different component columns
    /// - The lifetimes ensure proper borrowing semantics
    ///
    /// # Returns
    ///
    /// - `Some(Q)` if there are more entities to process
    /// - `None` if the iteration is complete
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.len {
            let table = self.source.table(self.table_ids[self.table_index]);
            let row = storage::Row::new(self.row_index);
            let entity = table.entity(row)?;

            self.row_index += 1;
            if self.row_index >= table.len() {
                self.table_index += 1;
                self.row_index = 0;
            }
            self.index += 1;

            let result = unsafe {
                D::fetch_mut(
                    entity,
                    // SAFETY: Creating aliased mutable table pointers is safe because each
                    // fetch_mut call accesses different component columns
                    &mut *(table as *mut storage::Table),
                    row,
                )
            };

            return result;
        }

        None
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<'w, D: Data> ExactSizeIterator for Result<'w, D> {}
