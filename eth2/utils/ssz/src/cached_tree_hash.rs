use hashing::hash;
use int_to_bytes::int_to_bytes32;
use std::iter::IntoIterator;
use std::iter::Iterator;
use std::ops::Range;
use std::vec::Splice;

mod impls;
// mod tests;

const BYTES_PER_CHUNK: usize = 32;
const HASHSIZE: usize = 32;
const MERKLE_HASH_CHUNCK: usize = 2 * BYTES_PER_CHUNK;

#[derive(Debug, PartialEq, Clone)]
pub enum Error {
    ShouldNotProduceBTreeOverlay,
    NoFirstNode,
    NoBytesForRoot,
    BytesAreNotEvenChunks(usize),
    NoModifiedFieldForChunk(usize),
    NoBytesForChunk(usize),
    NoChildrenForHashing((usize, usize)),
    CannotMerkleizeZeroLeaves,
    LeavesMustBePowerOfTwo,
}

pub trait CachedTreeHash {
    type Item: CachedTreeHash;

    fn build_tree_hash_cache(&self) -> Result<TreeHashCache, Error>;

    /// Return the number of bytes when this element is encoded as raw SSZ _without_ length
    /// prefixes.
    fn num_bytes(&self) -> usize;

    fn offsets(&self) -> Result<Vec<usize>, Error> {
        Err(Error::ShouldNotProduceBTreeOverlay)
    }

    fn num_packable_bytes(&self) -> usize;

    fn packable_bytes(
        &self,
        other: &Self::Item,
        cache: &mut TreeHashCache,
        offset: usize,
    ) -> Result<Vec<u8>, Error> {
        self.update_cache(other, cache, offset)?;

        let chunk = cache.get_chunk(offset)?;
        Ok(chunk[0..self.num_packable_bytes()].to_vec())
    }

    fn num_child_nodes(&self) -> usize {
        0
    }

    fn update_cache(
        &self,
        other: &Self::Item,
        cache: &mut TreeHashCache,
        cache_offset: usize,
    ) -> Result<usize, Error>;
}

#[derive(Debug, PartialEq, Clone)]
pub struct TreeHashCache {
    cache: Vec<u8>,
    chunk_modified: Vec<bool>,
}

impl Into<Vec<u8>> for TreeHashCache {
    fn into(self) -> Vec<u8> {
        self.cache
    }
}

impl TreeHashCache {
    pub fn new<T>(item: &T) -> Result<Self, Error>
    where
        T: CachedTreeHash,
    {
        item.build_tree_hash_cache()
    }

    pub fn from_leaves_and_subtrees<T>(
        item: &T,
        leaves_and_subtrees: Vec<Self>,
    ) -> Result<Self, Error>
    where
        T: CachedTreeHash,
    {
        let offset_handler = BTreeOverlay::new(item, 0)?;

        // Note how many leaves were provided. If is not a power-of-two, we'll need to pad it out
        // later.
        let num_provided_leaf_nodes = leaves_and_subtrees.len();

        // Allocate enough bytes to store the internal nodes and the leaves/subtrees, then fill
        // all the to-be-built internal nodes with zeros and append the leaves/subtrees.
        let internal_node_bytes = offset_handler.num_internal_nodes * BYTES_PER_CHUNK;
        let leaves_and_subtrees_bytes = leaves_and_subtrees
            .iter()
            .fold(0, |acc, t| acc + t.bytes_len());
        let mut cache = Vec::with_capacity(leaves_and_subtrees_bytes + internal_node_bytes);
        cache.resize(internal_node_bytes, 0);

        // Allocate enough bytes to store all the leaves.
        let mut leaves = Vec::with_capacity(offset_handler.num_leaf_nodes * HASHSIZE);

        // Iterate through all of the leaves/subtrees, adding their root as a leaf node and then
        // concatenating their merkle trees.
        for t in leaves_and_subtrees {
            leaves.append(&mut t.root()?);
            cache.append(&mut t.into_merkle_tree());
        }

        // Pad the leaves to an even power-of-two, using zeros.
        pad_for_leaf_count(num_provided_leaf_nodes, &mut cache);

        // Merkleize the leaves then split-off and discard the leaf nodes, keeping only the
        // internal nodes. Then, replace all-zeros internal nodes created earlier with those
        // generated by `merkleize`.
        let mut merkleized = merkleize(leaves)?;
        merkleized.split_off(internal_node_bytes);
        cache.splice(0..internal_node_bytes, merkleized);

        Ok(Self {
            chunk_modified: vec![false; cache.len() / BYTES_PER_CHUNK],
            cache,
        })
    }

    pub fn bytes_len(&self) -> usize {
        self.cache.len()
    }

    pub fn root(&self) -> Result<Vec<u8>, Error> {
        self.cache
            .get(0..HASHSIZE)
            .ok_or_else(|| Error::NoBytesForRoot)
            .and_then(|slice| Ok(slice.to_vec()))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() % BYTES_PER_CHUNK > 0 {
            return Err(Error::BytesAreNotEvenChunks(bytes.len()));
        }

        Ok(Self {
            chunk_modified: vec![false; bytes.len() / BYTES_PER_CHUNK],
            cache: bytes,
        })
    }

    /// Take the `root_chunk` and replace it with the hash of that chunk and the supplied `length`.
    pub fn mix_in_length(&mut self, root_chunk: usize, length: usize) -> Result<(), Error> {
        let mut mixed_in = self
            .get_chunk(root_chunk)
            .map_err(|_| Error::NoBytesForRoot)?
            .to_vec();
        mixed_in.append(&mut int_to_bytes32(length as u64));

        self.single_chunk_splice(root_chunk, hash(&mixed_in));

        Ok(())
    }

    pub fn single_chunk_splice<I>(&mut self, chunk: usize, replace_with: I) -> Splice<I::IntoIter>
    where
        I: IntoIterator<Item = u8>,
    {
        self.chunk_splice(chunk..chunk + 1, replace_with)
    }

    pub fn chunk_splice<I>(
        &mut self,
        chunk_range: Range<usize>,
        replace_with: I,
    ) -> Splice<I::IntoIter>
    where
        I: IntoIterator<Item = u8>,
    {
        let byte_start = chunk_range.start * BYTES_PER_CHUNK;
        let byte_end = chunk_range.end * BYTES_PER_CHUNK;

        self.cache.splice(byte_start..byte_end, replace_with)
    }

    pub fn chunk_slice_mut(&mut self, chunk_range: Range<usize>) -> Result<&mut [u8], Error> {
        let byte_start = chunk_range.start * BYTES_PER_CHUNK;
        let byte_end = chunk_range.end * BYTES_PER_CHUNK;

        self.cache
            .get_mut(byte_start..byte_end)
            .ok_or_else(|| Error::NoBytesForChunk(byte_end))
    }

    pub fn maybe_update_chunk(&mut self, chunk: usize, to: &[u8]) -> Option<()> {
        let start = chunk * BYTES_PER_CHUNK;
        let end = start + BYTES_PER_CHUNK;

        if !self.chunk_equals(chunk, to)? {
            self.cache.get_mut(start..end)?.copy_from_slice(to);
            self.chunk_modified[chunk] = true;
        }

        Some(())
    }

    pub fn modify_chunk(&mut self, chunk: usize, to: &[u8]) -> Result<(), Error> {
        let start = chunk * BYTES_PER_CHUNK;
        let end = start + BYTES_PER_CHUNK;

        self.cache
            .get_mut(start..end)
            .ok_or_else(|| Error::NoBytesForChunk(chunk))?
            .copy_from_slice(to);

        self.chunk_modified[chunk] = true;

        Ok(())
    }

    pub fn chunk_equals(&mut self, chunk: usize, other: &[u8]) -> Option<bool> {
        let start = chunk * BYTES_PER_CHUNK;
        let end = start + BYTES_PER_CHUNK;

        Some(self.cache.get(start..end)? == other)
    }

    pub fn get_chunk(&self, chunk: usize) -> Result<&[u8], Error> {
        let start = chunk * BYTES_PER_CHUNK;
        let end = start + BYTES_PER_CHUNK;

        self.cache
            .get(start..end)
            .ok_or_else(|| Error::NoBytesForChunk(chunk))
    }

    pub fn changed(&self, chunk: usize) -> Result<bool, Error> {
        self.chunk_modified
            .get(chunk)
            .cloned()
            .ok_or_else(|| Error::NoModifiedFieldForChunk(chunk))
    }

    pub fn either_modified(&self, children: (&usize, &usize)) -> Result<bool, Error> {
        Ok(self.changed(*children.0)? | self.changed(*children.1)?)
    }

    pub fn hash_children(&self, children: (&usize, &usize)) -> Result<Vec<u8>, Error> {
        let start = children.0 * BYTES_PER_CHUNK;
        let end = start + BYTES_PER_CHUNK * 2;

        let children = &self
            .cache
            .get(start..end)
            .ok_or_else(|| Error::NoChildrenForHashing((*children.0, *children.1)))?;

        Ok(hash(children))
    }

    /// Returns the total number of modifications made during the recent update.
    pub fn total_modifications(&self) -> usize {
        self.chunk_modified
            .iter()
            .fold(0, |acc, &b| acc + b as usize)
    }

    pub fn into_merkle_tree(self) -> Vec<u8> {
        self.cache
    }
}

fn children(parent: usize) -> (usize, usize) {
    ((2 * parent + 1), (2 * parent + 2))
}

fn num_nodes(num_leaves: usize) -> usize {
    2 * num_leaves - 1
}

#[derive(Debug)]
pub struct BTreeOverlay {
    num_internal_nodes: usize,
    num_leaf_nodes: usize,
    next_node: usize,
    offsets: Vec<usize>,
}

impl BTreeOverlay {
    pub fn new<T>(item: &T, initial_offset: usize) -> Result<Self, Error>
    where
        T: CachedTreeHash,
    {
        Self::from_lengths(initial_offset, item.offsets()?)
    }

    fn from_lengths(offset: usize, mut lengths: Vec<usize>) -> Result<Self, Error> {
        // Extend it to the next power-of-two, if it is not already.
        let num_leaf_nodes = if lengths.len().is_power_of_two() {
            lengths.len()
        } else {
            let num_leaf_nodes = lengths.len().next_power_of_two();
            lengths.resize(num_leaf_nodes, 1);
            num_leaf_nodes
        };

        let num_nodes = num_nodes(num_leaf_nodes);
        let num_internal_nodes = num_nodes - num_leaf_nodes;

        let mut offsets = Vec::with_capacity(num_nodes);
        offsets.append(&mut (offset..offset + num_internal_nodes).collect());

        let mut next_node = num_internal_nodes + offset;
        for i in 0..num_leaf_nodes {
            offsets.push(next_node);
            next_node += lengths[i];
        }

        Ok(Self {
            num_internal_nodes,
            num_leaf_nodes,
            offsets,
            next_node,
        })
    }

    pub fn total_nodes(&self) -> usize {
        self.num_internal_nodes + self.num_leaf_nodes
    }

    pub fn first_leaf_node(&self) -> Result<usize, Error> {
        self.offsets
            .get(self.num_internal_nodes)
            .cloned()
            .ok_or_else(|| Error::NoFirstNode)
    }

    pub fn next_node(&self) -> usize {
        self.next_node
    }

    pub fn iter_internal_nodes<'a>(
        &'a self,
    ) -> impl DoubleEndedIterator<Item = (&'a usize, (&'a usize, &'a usize))> {
        let internal_nodes = &self.offsets[0..self.num_internal_nodes];

        internal_nodes.iter().enumerate().map(move |(i, parent)| {
            let children = children(i);
            (
                parent,
                (&self.offsets[children.0], &self.offsets[children.1]),
            )
        })
    }

    pub fn iter_leaf_nodes<'a>(&'a self) -> impl DoubleEndedIterator<Item = &'a usize> {
        let leaf_nodes = &self.offsets[self.num_internal_nodes..];

        leaf_nodes.iter()
    }
}

/// Split `values` into a power-of-two, identical-length chunks (padding with `0`) and merkleize
/// them, returning the entire merkle tree.
///
/// The root hash is `merkleize(values)[0..BYTES_PER_CHUNK]`.
pub fn merkleize(mut values: Vec<u8>) -> Result<Vec<u8>, Error> {
    pad_leaves(&mut values);

    let leaves = values.len() / HASHSIZE;

    if leaves == 0 {
        return Err(Error::CannotMerkleizeZeroLeaves);
    }

    if !leaves.is_power_of_two() {
        return Err(Error::LeavesMustBePowerOfTwo);
    }

    let mut o: Vec<u8> = vec![0; (num_nodes(leaves) - leaves) * HASHSIZE];
    o.append(&mut values.to_vec());

    let mut i = o.len();
    let mut j = o.len() - values.len();

    while i >= MERKLE_HASH_CHUNCK {
        i -= MERKLE_HASH_CHUNCK;
        let hash = hash(&o[i..i + MERKLE_HASH_CHUNCK]);

        j -= HASHSIZE;
        o[j..j + HASHSIZE].copy_from_slice(&hash);
    }

    Ok(o)
}

pub fn pad_leaves(bytes: &mut Vec<u8>) {
    let present_leaves = num_unsanitized_leaves(bytes.len());
    let required_leaves = present_leaves.next_power_of_two();

    if (present_leaves != required_leaves) | last_leaf_needs_padding(bytes.len()) {
        bytes.resize(num_bytes(required_leaves), 0);
    }
}

fn pad_for_leaf_count(num_leaves: usize, bytes: &mut Vec<u8>) {
    let required_leaves = num_leaves.next_power_of_two();

    bytes.resize(
        bytes.len() + (required_leaves - num_leaves) * BYTES_PER_CHUNK,
        0,
    );
}

fn last_leaf_needs_padding(num_bytes: usize) -> bool {
    num_bytes % HASHSIZE != 0
}

/// Rounds up
fn num_unsanitized_leaves(num_bytes: usize) -> usize {
    (num_bytes + HASHSIZE - 1) / HASHSIZE
}

/// Rounds up
fn num_sanitized_leaves(num_bytes: usize) -> usize {
    let leaves = (num_bytes + HASHSIZE - 1) / HASHSIZE;
    leaves.next_power_of_two()
}

fn num_bytes(num_leaves: usize) -> usize {
    num_leaves * HASHSIZE
}
