use super::*;
use crate::{ssz_encode, Encodable};

impl CachedTreeHash<u64> for u64 {
    fn build_tree_hash_cache(&self) -> Result<TreeHashCache, Error> {
        let single_leaf = merkleize(ssz_encode(self))?;

        Ok(TreeHashCache::from_bytes(single_leaf)?)
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

    fn update_cache(
        &self,
        other: &u64,
        cache: &mut TreeHashCache,
        offset: usize,
    ) -> Result<usize, Error> {
        if self != other {
            let leaf = merkleize(ssz_encode(self))?;
            cache.modify_chunk(offset, &leaf)?;
        }

        Ok(offset + 1)
    }
}

impl<T> CachedTreeHash<Vec<T>> for Vec<T>
where
    T: CachedTreeHash<T> + Encodable + Sized,
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
            ItemType::Composite => {
                self
                    .iter()
                    .map(|item| {
                        item.offsets().iter().fold(0, |acc, o| acc + 0)
                    })
                    .collect()
            }
        };

        Ok(offsets)
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

    fn update_cache(
        &self,
        other: &Vec<T>,
        cache: &mut TreeHashCache,
        offset: usize,
    ) -> Result<usize, Error> {
        let num_packed_bytes = self.num_bytes();
        let num_leaves = num_sanitized_leaves(num_packed_bytes);
        if num_leaves != num_sanitized_leaves(other.num_bytes()) {
            panic!("Need to handle a change in leaf count");
        }

        // As `T::item_type()` is a static method on a type and a vec is a collection of identical
        // types, it's impossible for `Vec<T>` to contain two elements with a distinct `ItemType`.
        let leaves = match T::item_type() {
            ItemType::Basic => panic!("TODO"),
            ItemType::Composite => update_cache_of_composites(self, other, cache, offset)
        }?;

        /*
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

        Ok(next_node)
    }
}

fn get_leaves_for_composites<T>(
    new_vec: &Vec<T>,
    old_vec: &Vec<T>,
    cache: &mut TreeHashCache,
    offset: usize,
) -> Result<usize, Error>
where
    T: CachedTreeHash<T> + Encodable,
{
    assert_eq!(T::item_type(), ItemType::Composite);

    // Build an output vec with an appropriate capacity.
    let mut leaves = Vec::with_capacity(leaves_byte_len(new_vec));

    //  Build the leaves.
    for (i, item) in new_vec.iter().enumerate() {
        if i < old_vec.len() {
            offset = item.update_cache(&old_vec[i], cache, offset)?;
        } else {
            let cache = item.build_tree_hash_cache()?;
            leaves.append(&mut cache.into_merkle_tree());

            offset = overlay.next_node;
        }
    }
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

fn build_vec_leaves<T>(vec: &Vec<T>) -> Result<Vec<u8>, Error>
where
    T: CachedTreeHash<T> + Encodable,
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
fn leaves_byte_len<T>(vec: &Vec<T>) -> usize
where
    T: CachedTreeHash<T> + Encodable,
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
