//! A growable array with structural sharing, for mesh storage that undo history can share.

use std::fmt;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

/// A vector stored as fixed-size chunks behind [`Arc`]s, so clones share storage.
///
/// The table of chunk pointers is itself shared, so cloning is O(1). The first write after a
/// clone copies the table (one pointer per chunk) and the one chunk written
/// ([`Arc::make_mut`]); later writes to chunks this clone already owns happen in place. Reads
/// stay O(1), with two extra indirections over a `Vec`.
///
/// Elements are `Copy`, and the unused tail of the last chunk is filled with copies of the
/// element that started it, so no `Default` is needed.
#[derive(Clone)]
pub struct CowVec<T, const N: usize = 64> {
    #[expect(
        clippy::rc_buffer,
        reason = "the table is copied on write with Arc::make_mut, which needs a growable Vec"
    )]
    chunks: Arc<Vec<Arc<[T; N]>>>,
    len: usize,
}

impl<T, const N: usize> Default for CowVec<T, N> {
    fn default() -> Self {
        Self {
            chunks: Arc::new(Vec::new()),
            len: 0,
        }
    }
}

impl<T: Copy, const N: usize> CowVec<T, N> {
    /// Creates an empty vector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the vector has no elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends an element and returns its index.
    pub fn push(&mut self, value: T) -> usize {
        let index = self.len;
        let offset = index % N;
        let chunks = Arc::make_mut(&mut self.chunks);
        match chunks.last_mut() {
            Some(last) if offset != 0 => Arc::make_mut(last)[offset] = value,
            _ => chunks.push(Arc::new([value; N])),
        }
        self.len += 1;
        index
    }

    /// The element at `index`, or `None` if it is out of range.
    pub fn get(&self, index: usize) -> Option<&T> {
        (index < self.len).then(|| &self.chunks[index / N][index % N])
    }

    /// Iterates over the elements in order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &T> + Clone + '_ {
        (0..self.len).map(|i| &self.chunks[i / N][i % N])
    }
}

impl<T: Copy, const N: usize> Index<usize> for CowVec<T, N> {
    type Output = T;

    /// # Panics
    ///
    /// Panics if `index` is out of range.
    fn index(&self, index: usize) -> &T {
        assert!(
            index < self.len,
            "index {index} out of range for length {}",
            self.len
        );
        &self.chunks[index / N][index % N]
    }
}

impl<T: Copy, const N: usize> IndexMut<usize> for CowVec<T, N> {
    /// Copies the containing chunk first if another clone shares it.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of range.
    fn index_mut(&mut self, index: usize) -> &mut T {
        assert!(
            index < self.len,
            "index {index} out of range for length {}",
            self.len
        );
        &mut Arc::make_mut(&mut Arc::make_mut(&mut self.chunks)[index / N])[index % N]
    }
}

impl<T: Copy + fmt::Debug, const N: usize> fmt::Debug for CowVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Copy, const N: usize> FromIterator<T> for CowVec<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut v = Self::new();
        for x in iter {
            v.push(x);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Small = CowVec<u32, 4>;

    #[test]
    fn push_get_and_iterate_across_chunks() {
        let v: Small = (0..10).collect();
        assert_eq!(v.len(), 10);
        assert_eq!(v[0], 0);
        assert_eq!(v[9], 9);
        assert_eq!(v.get(10), None);
        assert_eq!(
            v.iter().copied().collect::<Vec<_>>(),
            (0..10).collect::<Vec<_>>()
        );
        assert_eq!(v.iter().len(), 10);
        assert_eq!(v.chunks.len(), 3);
    }

    #[test]
    fn clone_shares_chunks_until_written() {
        let a: Small = (0..10).collect();
        let mut b = a.clone();
        assert!(
            (0..3).all(|i| Arc::ptr_eq(&a.chunks[i], &b.chunks[i])),
            "clone shares every chunk"
        );

        b[5] = 100;
        assert_eq!(a[5], 5, "the original is unchanged");
        assert_eq!(b[5], 100);
        assert!(
            Arc::ptr_eq(&a.chunks[0], &b.chunks[0]),
            "untouched chunks stay shared"
        );
        assert!(
            !Arc::ptr_eq(&a.chunks[1], &b.chunks[1]),
            "the written chunk was copied"
        );
        assert!(
            Arc::ptr_eq(&a.chunks[2], &b.chunks[2]),
            "untouched chunks stay shared"
        );
    }

    #[test]
    fn clone_shares_the_chunk_table_until_written() {
        let a: Small = (0..10).collect();
        let mut b = a.clone();
        assert!(Arc::ptr_eq(&a.chunks, &b.chunks), "clone is O(1)");
        b[0] = 1;
        assert!(
            !Arc::ptr_eq(&a.chunks, &b.chunks),
            "first write copies the table"
        );
    }

    #[test]
    fn unshared_chunks_are_written_in_place() {
        let mut v: Small = (0..4).collect();
        let before = Arc::as_ptr(&v.chunks[0]);
        v[1] = 7;
        assert_eq!(Arc::as_ptr(&v.chunks[0]), before, "no copy without sharing");
    }

    #[test]
    fn pushing_onto_a_shared_tail_leaves_the_original_alone() {
        let a: Small = (0..6).collect();
        let mut b = a.clone();
        b.push(6);
        assert_eq!(a.len(), 6);
        assert_eq!(b.len(), 7);
        assert_eq!(b[6], 6);
        assert!(
            Arc::ptr_eq(&a.chunks[0], &b.chunks[0]),
            "full chunks stay shared"
        );
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn index_past_len_panics_even_inside_the_last_chunk() {
        let v: Small = (0..2).collect();
        let _ = v[3];
    }
}
