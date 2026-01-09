/// A storage row. A simple index into the table entity and column vecs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Row(usize);

impl From<usize> for Row {
    /// Get a row From a usize index.
    fn from(value: usize) -> Self {
        Self::new(value)
    }
}

impl Row {
    /// Construct a new table row from an index.
    #[inline]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Get the index used in the storage vecs.
    #[inline]
    pub fn index(&self) -> usize {
        self.0
    }

    /// Increment the row index by 1.
    #[inline]
    pub fn increment(&mut self) {
        self.0 += 1;
    }

    /// Decrement the row index by 1.
    #[inline]
    pub fn decrement(&mut self) {
        self.0 -= 1;
    }
}
