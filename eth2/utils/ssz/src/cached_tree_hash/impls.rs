use super::*;
use crate::{ssz_encode, Encodable};

impl CachedTreeHash<u64> for u64 {
    fn build_tree_hash_cache(&self) -> Result<TreeHashCache, Error> {
        let single_leaf = merkleize(ssz_encode(self))?;

        Ok(TreeHashCache::from_bytes(single_leaf)?)
    }

    fn packing_factor() -> usize {
        HASHSIZE / 8
    }

    fn num_packable_bytes(&self) -> usize {
        8
    }

    fn num_bytes(&self) -> usize {
        8
    }

    fn item_type() -> ItemType {
        ItemType::Basic
    }

    fn update_cache(&self, other: Option<&u64>, cache: &mut Vec<u8>, end: usize) -> Result<(usize, bool), Error> {
        let start = end
            .checked_sub(self.num_bytes())
            .ok_or_else(|| Error::BytesTooShort(end, cache.len()))?;

        let changed = if self != other {
            cache
                .get_mut(start..end)
                .ok_or_else(|| Error::UnableToGetBytes(start..end))?
                .copy_from_slice(&ssz_encode(self));
            true
        } else {
            false
        };

        Ok((start, changed))
    }
}

impl<T> CachedTreeHash<&[T]> for &[T]
where
    T: PartialEq<T> + CachedTreeHash<T> + Encodable + Sized,
{
    fn build_tree_hash_cache(&self) -> Result<TreeHashCache, Error> {
        let leaves = build_vec_leaves(self)?;
        let merkle_tree = merkleize(leaves)?;
        let mut cache = TreeHashCache::from_bytes(merkle_tree)?;

        cache.mix_in_length(0, self.len())?;

        Ok(cache)
    }

    fn item_type() -> ItemType {
        ItemType::Composite
    }

    fn offsets(&self) -> Result<Vec<usize>, Error> {
        // As `T::item_type()` is a static method on a type and a vec is a collection of identical
        // types, it's impossible for `Vec<T>` to contain two elements with a distinct `ItemType`.
        let offsets: Vec<usize> = match T::item_type() {
            ItemType::Basic => {
                let num_packed_bytes = self.num_bytes();
                let num_leaves = num_sanitized_leaves(num_packed_bytes);

                (0..num_leaves).collect()
            }
            ItemType::Composite => self
                .iter()
                .map(|item| item.offsets().iter().fold(0, |acc, o| acc + 0))
                .collect(),
        };

        Ok(offsets)
    }

    fn packing_factor() -> usize {
        1
    }

    fn num_packable_bytes(&self) -> usize {
        HASHSIZE
    }

    fn num_bytes(&self) -> usize {
        self.iter().fold(0, |acc, item| acc + item.num_bytes())
    }

    fn num_child_nodes(&self) -> usize {
        // TODO: this is probably wrong
        0
    }

    fn update_cache(&self, other: Option<&&[T]>, cache: &mut Vec<u8>, end: usize) -> Result<(usize, bool), Error> {
        // If this is the first build, use an empty vec as the previous value.
        let empty_vec: Vec<T> = vec![];
        let other = other.unwrap_or_else(|| &&empty_vec[..]);

        let num_packed_bytes = std::cmp::max(self.num_bytes(), other.num_bytes());
        let num_leaves = num_sanitized_leaves(num_packed_bytes);
        let num_nodes = num_nodes(num_leaves);
        let num_internal_nodes = num_nodes - num_leaves;

        let operations = vec![Op::NoOp; num_internal_nodes];

        let mut i = num_leaves;
        while i > 0 {
            let right = i;
            let left = i - 1;

            let (right_status, end) = node_status(
                self.chunks(T::packing_factor()).skip(right / 2).next(),
                other.chunks(T::packing_factor()).skip(right / 2).next(),
                cache,
                end,
            );
            let (left_status, end) = node_status(
                self.chunks(T::packing_factor()).skip(left / 2).next(),
                other.chunks(T::packing_factor()).skip(left / 2).next(),
                cache,
                end,
            );

            let parent_op = match (left_status, right_status) {
                (_, NodeStatus::ValueChanged) | (NodeStatus::ValueChanged, _) => {
                    // the value has changed. for basic, reserialize. for composite, rebuild.
                }
                (NodeStatus::BecameValue, _) => {
                    // A left node was created, a new parent needs to be created.
                }
                (_, NodeStatus::BecameValue) => {
                    // A right node was created, parent needs to be updated.
                }
                (NodeStatus::BecamePadding, _) => {
                    // A left node was removed, parent needs to be removed.
                }
                (_, NodeStatus::BecamePadding) => {
                    // A right node was removed, parent needs to be updated.
                }
                (NodeStatus::ValueUnchanged, NodeStatus::ValueUnchanged) => {
                    // Neither of the nodes changed, nothing to do.
                }
                (NodeStatus::RemainedPadding, NodeStatus::RemainedPadding) => {
                    // Neither of the nodes changed, nothing to do.
                }
                (NodeStatus::RemainedPadding, NodeStatus::ValueUnchanged) => {
                    unreachable!(
                        "Impossible for right node to have a value whilst left node is padding"
                    );
                }
                (NodeStatus::ValueUnchanged, NodeStatus::RemainedPadding) => {
                    // Neither of the nodes changed, nothing to do.
                }
            };

            i -= 2;
        }

        /*
        for i in (0..num_leaves).step_by(2).rev() {
            //
        }

        for (i, c) in (0..num_leaves)
            .collect::<Vec<usize>>()
            .chunks(2)
            .enumerate()
            .rev()
        {}

        let last_internal_node = {
            let num_packed_bytes = other.num_bytes();
            let num_leaves = num_sanitized_leaves(num_packed_bytes);
            last_node - num_leaves
        };
        */

        /*
        // As `T::item_type()` is a static method on a type and a vec is a collection of identical
        // types, it's impossible for `Vec<T>` to contain two elements with a distinct `ItemType`.
        let leaves = match T::item_type() {
            ItemType::Basic => panic!("TODO"),
            ItemType::Composite => update_cache_of_composites(self, other, cache, offset),
        }?;

        let overlay = BTreeOverlay::new(self, chunk)?;

        let unpadded_leaves = build_vec_leaves_with_cache(self, other, cache, overlay.first_leaf_node()?);

        // TODO: check lens are equal.

        for (i, item) in self.iter().enumerate() {
            leaves.append(&mut item.packable_bytes(&other[i], cache, offset)?);
            offset += 1;
        }
        */

        // Ensure the leaves are a power-of-two number of chunks
        // pad_leaves(&mut leaves);

        /*
        // TODO: try and avoid serializing all the leaves.
        let leaves = build_vec_leaves(self)?;

        {
            let mut chunk = chunk + num_internal_nodes;
            for new_chunk_bytes in packed.chunks(HASHSIZE) {
                cache.maybe_update_chunk(chunk, new_chunk_bytes)?;
                chunk += 1;
            }
        }

        // Iterate backwards through the internal nodes, rehashing any node where it's children
        // have changed.
        for chunk in (chunk..chunk + num_internal_nodes).into_iter().rev() {
            if cache.children_modified(chunk)? {
                cache.modify_chunk(chunk, &cache.hash_children(chunk)?)?;
            }
        }
        */

        Ok(42)
    }
}

fn process_values<T>(
    new: Option<&[T]>,
    old: Option<&[T]>,
    cache: &mut Vec<u8>,
    mut end: usize,
) -> (Op, usize)
where
    T: PartialEq<T> + CachedTreeHash<T>,
{
    if new.is_none() && old.is_none() {
        (Op::NoOp, end)
    } else if new.is_none() {
        (Op::Delete, end)
    } else {
        let mut modified = false;

        end -= HASHSIZE * (HASHSIZE / T::packing_factor() - new.unwrap().len());

        let len = std::cmp::max(new.len(), old.len());
        for i in 0..len.rev()  {
            (end, modified) = item.update_cache();
        }
    }


    else if old.is_none() {
        // TODO: update
        (Op::Insert, ???)
    } else if new == old {
        (NodeStatus::ValueUnchanged, )
    } else {
        NodeStatus::ValueChanged
    }

}

/*
fn node_status<T>(
    new: Option<&[T]>,
    old: Option<&[T]>,
    cache: &mut Vec<u8>,
    mut end: usize,
) -> (NodeStatus, usize)
where
    T: PartialEq<T> + CachedTreeHash<T>,
{
    if new.is_none() && old.is_none() {
        (NodeStatus::RemainedPadding, end)
    } else if new.is_none() {
        (NodeStatus::BecamePadding, end)
    } else if old.is_none() {
        (NodeStatus::BecameValue, ???)
    } else if new == old {
        end -= HASHSIZE * (HASHSIZE / T::packing_factor() - new.unwrap().len());
        for item in new.iter().rev() {
            end = item.update_cache()
        }
        (NodeStatus::ValueUnchanged, )
    } else {
        NodeStatus::ValueChanged
    }
}
*/

/*
fn get_leaves_for_composites<T>(
    new_vec: &Vec<T>,
    old_vec: &Vec<T>,
    cache: &mut TreeHashCache,
    mut offset: usize,
) -> Result<usize, Error>
where
    T: CachedTreeHash<T> + Encodable,
{
    assert_eq!(T::item_type(), ItemType::Composite);

    if num_sanitized_leaves(old_vec.num_bytes()) != num_sanitized_leaves(new_vec.num_bytes()) {
        // TODO: deal with case where sanitized number of leaves changes.
        return Ok(42);
    }

    // Build an output vec with an appropriate capacity.
    // let mut leaves = Vec::with_capacity(leaves_byte_len(new_vec));

    // Iterate through the new vec and update each element.
    for (i, item) in new_vec.iter().enumerate() {
        if i < old_vec.len() {
            offset = item.update_cache(&old_vec[i], cache, offset)?;
        } else {
            // Build a new element from scratch.
            let item_cache = item.build_tree_hash_cache()?;
            // Insert it into the tree.
            cache.chunk_splice(offset..offset, item_cache.into_merkle_tree());

            offset = BTreeOverlay::new(item, offset)?.next_node;
        }
    }

    // Iterate through elements that exist in `old_vec` but not in `new_vec`.
    if new_vec.len() < old_vec.len() {
        let overlays_to_remove = old_vec
            .iter()
            .skip(new_vec.len())
            .map(|item| {
                let overlay = BTreeOverlay::new(item, offset)?;
                offset = overlay.next_node;
                Ok(overlay)
            })
            .collect::<Result<Vec<BTreeOverlay>, _>>()?;

        let first_node = overlays_to_remove
            .first()
            .expect("List cannot be empty.")
            .first_node()?;
        let last_node = offset;

        cache.chunk_splice(*first_node..last_node, vec![]);
    }

    Ok(offset)
}

fn build_vec_leaves_with_cache<T>(
    new_vec: &Vec<T>,
    old_vec: &Vec<T>,
    cache: &mut TreeHashCache,
    mut offset: usize,
) -> Result<Vec<u8>, Error>
where
    T: CachedTreeHash<T> + Encodable,
{
    // Build an output vec with an appropriate capacity.
    let mut leaves = Vec::with_capacity(leaves_byte_len(new_vec));

    //  Build the leaves.
    for (i, item) in new_vec.iter().enumerate() {
        // As `T::item_type()` is a static method on a type and a vec is a collection of identical
        // types, it's impossible for `Vec<T>` to contain two elements with a distinct `ItemType`.
        match T::item_type() {
            ItemType::Basic => leaves.append(&mut ssz_encode(item)),
            ItemType::Composite => {
                if i < old_vec.len() {
                    offset = item.update_cache(&old_vec[i], cache, offset)?;
                } else {
                    let cache = item.build_tree_hash_cache()?;
                    leaves.append(&mut cache.into_merkle_tree());

                    // Determine how many nodes were added by initializing a an overlay.
                    let overlay = BTreeOverlay::new(item, offset)?;

                    offset = overlay.next_node;
                }
            }
        }
    }

    // Ensure the leaves are a power-of-two number of chunks
    // pad_leaves(&mut leaves);

    Ok(leaves)
}
*/

fn build_vec_leaves<T>(vec: &[T]) -> Result<Vec<u8>, Error>
where
    T: CachedTreeHash<T> + PartialEq<T> + Encodable,
{
    // Build an output vec with an appropriate capacity.
    let mut leaves = Vec::with_capacity(leaves_byte_len(vec));

    //  Build the leaves.
    for item in vec {
        leaves.append(&mut ssz_encode(item));
    }

    // Ensure the leaves are a power-of-two number of chunks
    pad_leaves(&mut leaves);

    Ok(leaves)
}

/// Returns the number of bytes required to store the leaves for some `vec`.
fn leaves_byte_len<T>(vec: &[T]) -> usize
where
    T: CachedTreeHash<T> + PartialEq<T> + Encodable,
{
    let num_packed_bytes = vec.num_bytes();
    let num_leaves = num_sanitized_leaves(num_packed_bytes);
    num_leaves * HASHSIZE
}

/*
fn merkleize_vec<T>(vec: &Vec<T>) -> Result<Vec<u8>, Error>
where
    T: CachedTreeHash + Encodable,
{
    // Build an output vec with an appropriate capacity.
    let num_packed_bytes = vec.num_bytes();
    let num_leaves = num_sanitized_leaves(num_packed_bytes);
    let mut leaves = Vec::with_capacity(num_leaves * HASHSIZE);

    //  Build the leaves.
    for item in vec {
        leaves.append(&mut ssz_encode(item));
    }

    merkleize(leaves)
}
*/
