//! Exact immutable row-list suffixes for recovery descriptors.
//!
//! Chunking starts at the tail, so a previously interned suffix is reused even
//! when another site prepends different roots. A node only names an already
//! allocated child. List IDs are private compile-time identities; append lowers
//! them to immutable absolute word addresses for the module's constant getter.

use std::collections::BTreeMap;

const CHUNK: usize = 8;

#[derive(Clone)]
struct Node {
    next: u32,
    rows: Vec<u32>,
}

#[derive(Clone, Default)]
pub(super) struct ListPool {
    nodes: Vec<Node>,
    interned: BTreeMap<(u32, Vec<u32>), u32>,
}

impl ListPool {
    /// Empty is ID0. All other identities are one-based, and exact `(next,
    /// rows)` equality is the only sharing criterion.
    pub fn intern(&mut self, rows: &[u32]) -> u32 {
        let mut next = 0;
        for chunk in rows.rchunks(CHUNK) {
            let key = (next, chunk.to_vec());
            next = if let Some(&id) = self.interned.get(&key) {
                id
            } else {
                let id = u32::try_from(self.nodes.len())
                    .ok()
                    .and_then(|length| length.checked_add(1))
                    .expect("bounded recovery row-list identities");
                debug_assert!(next < id);
                self.nodes.push(Node {
                    next,
                    rows: key.1.clone(),
                });
                self.interned.insert(key, id);
                id
            };
        }
        next
    }

    /// Append `[next-address, length, row-address...]` in allocation order.
    /// `row_base` and returned addresses are word indices, not byte pointers.
    /// The returned index0 always denotes the empty list at address0.
    pub fn append(&self, words: &mut Vec<i64>, row_base: u32) -> Vec<u32> {
        let mut addresses = Vec::with_capacity(self.nodes.len() + 1);
        addresses.push(0);
        if self.nodes.is_empty() {
            return addresses;
        }

        // Keep address0 exclusively for the empty sentinel, including when a
        // test/compiler client starts with an otherwise empty constant table.
        let reserve_zero = words.is_empty();
        let mut cursor = words
            .len()
            .checked_add(usize::from(reserve_zero))
            .expect("bounded recovery row-list table");
        for (index, node) in self.nodes.iter().enumerate() {
            assert!(
                node.next as usize <= index,
                "row-list children are allocated first"
            );
            assert!(!node.rows.is_empty() && node.rows.len() <= CHUNK);
            addresses.push(u32::try_from(cursor).expect("bounded recovery row-list address"));
            cursor = cursor
                .checked_add(2 + node.rows.len())
                .expect("bounded recovery row-list table");
            u32::try_from(cursor).expect("bounded recovery row-list table");
            for &row in &node.rows {
                row.checked_mul(2)
                    .and_then(|offset| row_base.checked_add(offset))
                    .expect("bounded recovery row-dictionary address");
            }
        }

        // All address arithmetic is checked before mutating the destination.
        if reserve_zero {
            words.push(0);
        }
        for (index, node) in self.nodes.iter().enumerate() {
            debug_assert_eq!(words.len(), addresses[index + 1] as usize);
            words.push(i64::from(addresses[node.next as usize]));
            words.push(node.rows.len() as i64);
            for &row in &node.rows {
                words.push(i64::from(row_base + row * 2));
            }
        }
        addresses
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(words: &[i64], mut address: u32, row_base: u32) -> Vec<u32> {
        let mut result = Vec::new();
        while address != 0 {
            let index = address as usize;
            let next = words[index] as u32;
            let length = words[index + 1] as usize;
            assert!((1..=CHUNK).contains(&length));
            for &row in &words[index + 2..index + 2 + length] {
                let offset = u32::try_from(row).unwrap().checked_sub(row_base).unwrap();
                assert_eq!(offset % 2, 0);
                result.push(offset / 2);
            }
            assert!(
                next < address,
                "encoded child addresses precede their parent"
            );
            address = next;
        }
        result
    }

    #[test]
    fn empty_list_is_zero_and_does_not_append_or_allocate() {
        let mut pool = ListPool::default();
        assert_eq!(pool.intern(&[]), 0);
        assert!(pool.nodes.is_empty());
        let mut words = vec![11, 22];
        assert_eq!(pool.append(&mut words, 100), vec![0]);
        assert_eq!(words, [11, 22]);
        let mut empty = Vec::new();
        assert_eq!(pool.append(&mut empty, 0), vec![0]);
        assert!(empty.is_empty());
    }

    #[test]
    fn exact_list_reuses_every_node_and_different_order_is_distinct() {
        let mut pool = ListPool::default();
        let first = pool.intern(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let count = pool.nodes.len();
        assert_eq!(pool.intern(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), first);
        assert_eq!(pool.nodes.len(), count);
        assert_ne!(pool.intern(&[2, 1, 3, 4, 5, 6, 7, 8, 9, 10]), first);
    }

    #[test]
    fn prepended_roots_share_the_exact_eight_row_suffix() {
        let mut pool = ListPool::default();
        let suffix = pool.intern(&[3, 4, 5, 6, 7, 8, 9, 10]);
        let first = pool.intern(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let second = pool.intern(&[21, 22, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(pool.nodes.len(), 3);
        assert_eq!(pool.nodes[first as usize - 1].next, suffix);
        assert_eq!(pool.nodes[second as usize - 1].next, suffix);
        assert_ne!(first, second);
    }

    #[test]
    fn chunk_order_and_repeated_row_ids_round_trip_without_sorting() {
        let mut pool = ListPool::default();
        let expected = [9, 2, 2, 1, 8, 7, 6, 5, 4, 3, 1, 0, 11, 12, 13, 14, 15];
        let id = pool.intern(&expected);
        assert_eq!(pool.nodes.len(), 3);
        let mut words = vec![0];
        let addresses = pool.append(&mut words, 300);
        assert_eq!(rows(&words, addresses[id as usize], 300), expected);
        assert!(
            pool.nodes
                .iter()
                .enumerate()
                .all(|(index, node)| node.next as usize <= index)
        );
    }

    #[test]
    fn dense_append_preserves_prefix_and_uses_absolute_word_addresses() {
        let mut pool = ListPool::default();
        let suffix = pool.intern(&[7, 8]);
        let other = pool.intern(&[5]);
        let mut words = vec![90, 91, 92];
        let addresses = pool.append(&mut words, 100);
        assert_eq!(addresses, [0, 3, 7]);
        assert_eq!(words, [90, 91, 92, 0, 2, 114, 116, 0, 1, 110]);
        assert_eq!(rows(&words, addresses[suffix as usize], 100), [7, 8]);
        assert_eq!(rows(&words, addresses[other as usize], 100), [5]);
    }

    #[test]
    fn empty_destination_reserves_address_zero_and_repeated_append_is_immutable() {
        let mut pool = ListPool::default();
        let id = pool.intern(&[4]);
        let mut first = Vec::new();
        let mut second = Vec::new();
        let a = pool.append(&mut first, 200);
        let b = pool.append(&mut second, 200);
        assert_eq!(a, [0, 1]);
        assert_eq!(a, b);
        assert_eq!(first, second);
        assert_eq!(first, [0, 0, 1, 208]);
        assert_eq!(rows(&first, a[id as usize], 200), [4]);
    }

    #[test]
    fn overflowing_row_address_is_rejected_before_any_append() {
        let mut pool = ListPool::default();
        pool.intern(&[1]);
        let mut words = vec![10, 20];
        let before = words.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pool.append(&mut words, u32::MAX);
        }));
        assert!(result.is_err());
        assert_eq!(words, before);
    }
}
