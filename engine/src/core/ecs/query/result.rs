use std::marker::PhantomData;

use crate::core::ecs::{
    query::data::{Data, DataSpec},
    storage,
    world::World,
};

pub struct Result<'w, Q: Data<'w>> {
    world: &'w mut World,
    query_spec: DataSpec,
    table_ids: Vec<storage::table::Id>,
    table_index: usize,
    row_index: usize,
    index: usize,
    len: usize,
    _marker: PhantomData<Q>,
}

impl<'w, Q: Data<'w>> Result<'w, Q> {
    #[inline]
    pub fn new(
        world: &'w mut World,
        query_spec: DataSpec,
        table_ids: Vec<storage::table::Id>,
    ) -> Self {
        // Pre-calculate the total length.
        let mut len = 0;
        for table_id in table_ids.iter() {
            // Safety - We know this is a valid table as we got this ID from the registry
            // before creating this result.
            len += world.storage().get(*table_id).entities().len();
        }

        Self {
            world,
            query_spec,
            table_ids,
            table_index: 0,
            row_index: 0,
            len,
            index: 0,
            _marker: PhantomData,
        }
    }
}

impl<'w, Q: Data<'w>> Iterator for Result<'w, Q> {
    type Item = Q;

    fn next(&mut self) -> Option<Self::Item> {
        println!("AI: {:?}, RI: {:?}", self.table_ids, self.row_index);
        if self.index < self.len {
            let table = self
                .world
                .storage_mut()
                .get_mut(self.table_ids[self.table_index]);
            let row = storage::Row::new(self.row_index);
            let entity = table.entity(row)?;
            println!(
                "Found Table: {:?}-{:?}, {:?}",
                table.id(),
                table.len(),
                table.components()
            );

            self.row_index += 1;
            if self.row_index >= table.len() {
                self.table_index += 1;
                self.row_index = 0;
            }
            self.index += 1;

            let result = unsafe {
                Q::fetch(
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

impl<'w, Q: Data<'w>> ExactSizeIterator for Result<'w, Q> {}
