use std::any::TypeId;

use crate::ecs::{
    component::{self, Component},
    storage::{column::Column, Row, Table},
    world,
};

/// Trait for values that can be written to a table column.
///
/// This unifies typed component writes and byte-based writes under a single interface,
/// allowing both paths to use the same `Table::apply_column_write` method.
///
/// # Implementations
///
/// - `C: Component` - Zero-cost typed writes using compile-time type info
/// - `ExtractedValue` - Byte-based writes for migrated component data
pub trait ColumnWrite {
    /// The `std::any::TypeId` for column lookup.
    fn type_id(&self) -> TypeId;

    /// Write self to the column at the given row.
    ///
    /// # Safety
    /// Caller must ensure:
    /// - Row is within column's reserved capacity
    /// - The data matches the column's component type
    unsafe fn write_to_column(self, column: &mut Column, row: Row);
}

/// Typed components use zero-cost writes with compile-time type information.
impl<C: Component> ColumnWrite for C {
    #[inline]
    fn type_id(&self) -> TypeId {
        TypeId::of::<C>()
    }

    #[inline]
    unsafe fn write_to_column(self, column: &mut Column, row: Row) {
        // SAFETY: Caller guarantees row is valid and type matches
        unsafe { column.write(row, self) }
    }
}

/// A value extracted from a column during entity migration.
///
/// Contains both ID types to support different use cases:
/// - `world_id`: For `Spec` creation and archetype matching
/// - `type_id`: For column lookup during writes
pub struct ExtractedValue {
    /// ECS type ID (for Spec creation, archetype matching)
    pub world_id: world::TypeId,
    /// Rust type ID (for column lookup)
    pub type_id: TypeId,
    /// Raw component bytes
    pub bytes: Vec<u8>,
}

impl ColumnWrite for ExtractedValue {
    #[inline]
    fn type_id(&self) -> TypeId {
        self.type_id
    }

    #[inline]
    unsafe fn write_to_column(self, column: &mut Column, row: Row) {
        // SAFETY: Caller guarantees row is valid and bytes match column layout
        unsafe { column.write_bytes(row, &self.bytes) }
    }
}

/// Collection of extracted component values from a table row.
///
/// Used during entity migration to transfer component data between tables
/// without dropping and recreating the values.
pub type Extract = Vec<ExtractedValue>;

impl component::Set for Extract {
    fn as_spec(&self, _registry: &world::TypeRegistry) -> component::Spec {
        // Use world_id for spec creation (archetype matching)
        component::Spec::new(self.iter().map(|v| v.world_id).collect::<Vec<_>>())
    }

    fn apply(self, target: &mut Table, row: Row) {
        for value in self {
            target.apply_column_write(row, value);
        }
    }
}
