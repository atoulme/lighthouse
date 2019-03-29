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

    fn packable_bytes(
        &self,
        _other: &u64,
        _cache: &mut TreeHashCache,
        _offset: usize,
    ) -> Result<Vec<u8>, Error> {
        // Ideally we would try and read from the cache here, however we skip that for simplicity.
        Ok(ssz_encode(self))
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
        chunk: usize,
    ) -> Result<usize, Error> {
        let num_packed_bytes = self.num_bytes();
        let num_leaves = num_sanitized_leaves(num_packed_bytes);
        if num_leaves != num_sanitized_leaves(other.num_bytes()) {
            panic!("Need to handle a change in leaf count");
        }

        let _overlay = BTreeOverlay::new(self, chunk)?;

        // Build an output vec with an appropriate capacity.
        let mut leaves = Vec::with_capacity(leaves_byte_len(self));

        // TODO: check lens are equal.

        //  Build the leaves.
        let mut offset = chunk;
        for i in 0..self.len() {
            leaves.append(&mut self[i].packable_bytes(&other[i], cache, offset)?);
            offset += 1;
        }
        /*
        for (i, item) in self.iter().enumerate() {
            leaves.append(&mut item.packable_bytes(&other[i], cache, offset)?);
            offset += 1;
        }
        */

        // Ensure the leaves are a power-of-two number of chunks
        pad_leaves(&mut leaves);

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

        Ok(chunk + 42)
    }
}

/*
fn build_vec_leaves_with_cache<T>(
    vec: &Vec<T::Item>,
    other_vec: &Vec<T::Item>,
    cache: &mut TreeHashCache,
    mut offset: usize,
) -> Result<Vec<u8>, Error>
where
    T: CachedTreeHash + Encodable,
    <T as CachedTreeHash>::Item: CachedTreeHash + Encodable,
{
    // Build an output vec with an appropriate capacity.
    let mut leaves = Vec::with_capacity(leaves_byte_len(vec));

    // TODO: check lens are equal.

    //  Build the leaves.
    for (i, item) in vec.iter().enumerate() {
        leaves.append(&mut item.packable_bytes(&other_vec[i], cache, offset)?);
        offset += 1;
    }

    // Ensure the leaves are a power-of-two number of chunks
    pad_leaves(&mut leaves);

    Ok(leaves)
}
*/

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
