use crate::ecs::{component, storage, world};

/// A trait representing a data source for query execution. Implementors of this trait
/// should provide access to component storage tables and enforce access control by evaluating
/// access requests.
///
/// Known implementors include `world::World` and `world::Shard`.
pub trait DataSource {
    /// Determines if the data source allows access based on the provided access request.
    fn allows(&self, request: &world::AccessRequest) -> bool;

    /// Get the ids for component storage tables that support the given component
    /// specification.
    fn table_ids_for(&self, components: &component::Spec) -> Vec<storage::TableId>;

    /// Get access to a specific component storage table by its ID.  The assumes the caller's
    /// access has already been validated via `allows()`.
    fn table(&mut self, table_id: storage::TableId) -> &mut storage::Table;
}

/// Implement DataSource for the ECS world.
impl DataSource for world::World {
    /// The world always allows access to all resources.
    #[inline]
    fn allows(&self, _request: &world::AccessRequest) -> bool {
        true
    }

    /// Gets the table IDs for the given component specification.
    #[inline]
    fn table_ids_for(&self, components: &component::Spec) -> Vec<storage::TableId> {
        self.archetypes().table_ids_for(components)
    }

    #[inline]
    fn table(&mut self, table_id: storage::TableId) -> &mut storage::Table {
        // Safety: Caller must ensure access is valid via allows()
        self.storage_mut().get_table_mut(table_id)
    }
}

/// Implement DataSource for a world shard.
impl DataSource for world::Shard<'_> {
    /// The world always allows access to all resources.
    #[inline]
    fn allows(&self, request: &world::AccessRequest) -> bool {
        self.grant().grants(request)
    }

    /// Gets the table IDs for the given component specification.
    #[inline]
    fn table_ids_for(&self, components: &component::Spec) -> Vec<storage::TableId> {
        self.archetypes().table_ids_for(components)
    }

    #[inline]
    fn table(&mut self, table_id: storage::TableId) -> &mut storage::Table {
        // Safety: Caller must ensure access is valid via allows()
        self.storage_mut().get_table_mut(table_id)
    }
}
