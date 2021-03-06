//! Common logic for the consolidation of vectors of MonoidValues.
//!
//! Often we find ourselves with collections of records with associated weights
//! (often integers) where we want to reduce the collection to the point that
//! each record occurs at most once, with the accumulated weights. These methods
//! supply that functionality.

use crate::algebra::{AddAssignByRef, HasZero, MonoidValue};
use std::ptr;

/// Sorts and consolidates `vec`.
///
/// This method will sort `vec` and then consolidate runs of more than one entry
/// with identical first elements by accumulating the second elements of the
/// pairs. Should the final accumulation be zero, the element is discarded.
pub fn consolidate<T, R>(vec: &mut Vec<(T, R)>)
where
    T: Ord,
    R: MonoidValue,
{
    consolidate_from(vec, 0);
}

/// Sorts and consolidate `vec[offset..]`.
///
/// This method will sort `vec[offset..]` and then consolidate runs of more than
/// one entry with identical first elements by accumulating the second elements
/// of the pairs. Should the final accumulation be zero, the element is
/// discarded.
pub fn consolidate_from<T, R>(vec: &mut Vec<(T, R)>, offset: usize)
where
    T: Ord,
    R: MonoidValue,
{
    let length = consolidate_slice(&mut vec[offset..]);
    vec.truncate(offset + length);
}

/// Sorts and consolidates a slice, returning the valid prefix length.
// TODO: I'm pretty sure there's some improvements to be made here.
//       We don't really need (pure) slice consolidation from what I've
//       seen, we only actually care about consolidating vectors and
//       portions *of* vectors, so taking a starting index and a vector
//       would allow us to operate over the vec with the ability to discard
//       elements, meaning that we could drop elements instead of swapping
//       them once their diff hits zero. Is that significant? I don't really
//       know, but ~1 second to consolidate 10 million elements is
//       nearly intolerable, combining the sorting and compacting processes
//       could help alleviate that though.
pub fn consolidate_slice<T, R>(slice: &mut [(T, R)]) -> usize
where
    T: Ord,
    R: AddAssignByRef + HasZero,
{
    // We could do an insertion-sort like initial scan which builds up sorted,
    // consolidated runs. In a world where there are not many results, we may
    // never even need to call in to merge sort.
    slice.sort_by(|(key1, _), (key2, _)| key1.cmp(key2));

    let slice_ptr = slice.as_mut_ptr();

    // Counts the number of distinct known-non-zero accumulations. Indexes the write
    // location.
    let mut offset = 0;
    for index in 1..slice.len() {
        // The following unsafe block elides various bounds checks, using the reasoning
        // that `offset` is always strictly less than `index` at the beginning
        // of each iteration. This is initially true, and in each iteration
        // `offset` can increase by at most one (whereas `index` always
        // increases by one). As `index` is always in bounds, and `offset` starts at
        // zero, it too is always in bounds.
        //
        // LLVM appears to struggle to optimize out Rust's split_at_mut, which would
        // prove disjointness using run-time tests.
        unsafe {
            debug_assert!(offset < index);

            // LOOP INVARIANT: offset < index
            let ptr1 = slice_ptr.add(offset);
            let ptr2 = slice_ptr.add(index);

            if (*ptr1).0 == (*ptr2).0 {
                (*ptr1).1.add_assign_by_ref(&(*ptr2).1);
            } else {
                if !(*ptr1).1.is_zero() {
                    offset += 1;
                }

                let ptr1 = slice_ptr.add(offset);
                ptr::swap(ptr1, ptr2);
            }
        }
    }

    if offset < slice.len() && !slice[offset].1.is_zero() {
        offset += 1;
    }

    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consolidate() {
        let test_cases = vec![
            (vec![("a", -1), ("b", -2), ("a", 1)], vec![("b", -2)]),
            (vec![("a", -1), ("b", 0), ("a", 1)], vec![]),
            (vec![("a", 0)], vec![]),
            (vec![("a", 0), ("b", 0)], vec![]),
            (vec![("a", 1), ("b", 1)], vec![("a", 1), ("b", 1)]),
        ];

        for (mut input, output) in test_cases {
            consolidate(&mut input);
            assert_eq!(input, output);
        }
    }

    #[cfg_attr(miri, ignore)]
    mod proptests {
        use crate::{trace::consolidation::consolidate, utils::VecExt};
        use proptest::{collection::vec, prelude::*};
        use std::collections::BTreeMap;

        prop_compose! {
            /// Create a batch data tuple
            fn tuple()(key in 0..10_000usize, value in 0..10_000usize, diff in -10_000..=10_000isize) -> ((usize, usize), isize) {
                ((key, value), diff)
            }
        }

        prop_compose! {
            /// Generate a random batch of data
            fn batch()
                (length in 0..50_000)
                (batch in vec(tuple(), 0..=length as usize))
            -> Vec<((usize, usize), isize)> {
                batch
            }
        }

        fn batch_data(batch: &[((usize, usize), isize)]) -> BTreeMap<(usize, usize), i64> {
            let mut values = BTreeMap::new();
            for &(tuple, diff) in batch {
                values
                    .entry(tuple)
                    .and_modify(|acc| *acc += diff as i64)
                    .or_insert(diff as i64);
            }

            // Elements with a value of zero are removed in consolidation
            values.retain(|_, &mut diff| diff != 0);
            values
        }

        proptest! {
            #[test]
            fn consolidate_batch(mut batch in batch()) {
                let input = batch_data(&batch);
                consolidate(&mut batch);
                let output = batch_data(&batch);

                // Ensure the batch is sorted
                prop_assert!(batch.is_sorted_by(|(a, _), (b, _)| a.partial_cmp(b)));
                // Ensure no diff values are zero
                prop_assert!(batch.iter().all(|&(_, diff)| diff != 0));
                // Ensure the aggregated data is the same
                prop_assert_eq!(input, output);
            }
        }
    }
}
