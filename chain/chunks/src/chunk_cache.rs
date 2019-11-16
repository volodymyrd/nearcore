use std::collections::HashMap;

use cached::{Cached, SizedCache};
use near_primitives::hash::CryptoHash;
use near_primitives::sharding::{
    ChunkHash, PartialEncodedChunk, PartialEncodedChunkPart, ReceiptProof, ShardChunkHeader,
};
use near_primitives::types::{BlockIndex, ShardId};

const HEIGHT_HORIZON: u64 = 1024;
const MAX_HEIGHTS_AHEAD: u64 = 5;
const NUM_BLOCK_HASH_TO_CHUNK_HEADER: usize = 10;

pub struct EncodedChunksCacheEntry {
    pub header: ShardChunkHeader,
    pub parts: HashMap<u64, PartialEncodedChunkPart>,
    pub receipts: HashMap<ShardId, ReceiptProof>,
}

pub struct EncodedChunksCache {
    largest_seen_height: BlockIndex,

    encoded_chunks: HashMap<ChunkHash, EncodedChunksCacheEntry>,
    height_map: HashMap<BlockIndex, Vec<ChunkHash>>,
    block_hash_to_chunk_headers: SizedCache<CryptoHash, Vec<(ShardId, ShardChunkHeader)>>,
}

impl EncodedChunksCacheEntry {
    pub fn from_chunk_header(header: ShardChunkHeader) -> Self {
        EncodedChunksCacheEntry { header, parts: HashMap::new(), receipts: HashMap::new() }
    }

    pub fn merge_in_partial_encoded_chunk(&mut self, partial_encoded_chunk: &PartialEncodedChunk) {
        for part_info in partial_encoded_chunk.parts.iter() {
            let part_ord = part_info.part_ord;
            if !self.parts.contains_key(&part_ord) {
                self.parts.insert(part_ord, part_info.clone());
            }
        }

        for receipt in partial_encoded_chunk.receipts.iter() {
            let shard_id = receipt.1.to_shard_id;
            if !self.receipts.contains_key(&shard_id) {
                self.receipts.insert(shard_id, receipt.clone());
            }
        }
    }
}

impl EncodedChunksCache {
    pub fn new() -> Self {
        EncodedChunksCache {
            largest_seen_height: 0,
            encoded_chunks: HashMap::new(),
            height_map: HashMap::new(),
            block_hash_to_chunk_headers: SizedCache::with_size(NUM_BLOCK_HASH_TO_CHUNK_HEADER),
        }
    }

    pub fn get(&self, chunk_hash: &ChunkHash) -> Option<&EncodedChunksCacheEntry> {
        self.encoded_chunks.get(&chunk_hash)
    }

    pub fn remove(&mut self, chunk_hash: &ChunkHash) -> Option<EncodedChunksCacheEntry> {
        self.encoded_chunks.remove(&chunk_hash)
    }

    pub fn insert(&mut self, chunk_hash: ChunkHash, entry: EncodedChunksCacheEntry) {
        self.encoded_chunks.insert(chunk_hash, entry);
    }

    // `chunk_header` must be `Some` if the entry is absent, caller must ensure that
    pub fn get_or_insert_from_header(
        &mut self,
        chunk_hash: ChunkHash,
        chunk_header: Option<&ShardChunkHeader>,
    ) -> &mut EncodedChunksCacheEntry {
        self.encoded_chunks.entry(chunk_hash).or_insert_with(|| {
            EncodedChunksCacheEntry::from_chunk_header(chunk_header.unwrap().clone())
        })
    }

    pub fn height_within_horizon(&self, height: BlockIndex) -> bool {
        if height + HEIGHT_HORIZON < self.largest_seen_height {
            false
        } else if height > self.largest_seen_height + MAX_HEIGHTS_AHEAD {
            false
        } else {
            true
        }
    }

    pub fn merge_in_partial_encoded_chunk(
        &mut self,
        partial_encoded_chunk: &PartialEncodedChunk,
    ) -> bool {
        let chunk_hash = partial_encoded_chunk.chunk_hash.clone();
        if self.encoded_chunks.contains_key(&chunk_hash) || partial_encoded_chunk.header.is_some() {
            self.get_or_insert_from_header(chunk_hash, partial_encoded_chunk.header.as_ref())
                .merge_in_partial_encoded_chunk(&partial_encoded_chunk);
            return true;
        } else {
            return false;
        }
    }

    pub fn remove_from_cache_if_outside_horizon(&mut self, chunk_hash: &ChunkHash) {
        if let Some(entry) = self.encoded_chunks.get(chunk_hash) {
            let height = entry.header.inner.height_created;
            if !self.height_within_horizon(height) {
                self.encoded_chunks.remove(chunk_hash);
            }
        }
    }

    pub fn update_largest_seen_height<T>(
        &mut self,
        new_height: BlockIndex,
        requested_chunks: &mut SizedCache<ChunkHash, T>,
    ) {
        let old_largest_seen_height = self.largest_seen_height;
        self.largest_seen_height = new_height;
        for height in old_largest_seen_height.saturating_sub(HEIGHT_HORIZON)
            ..self.largest_seen_height.saturating_sub(HEIGHT_HORIZON)
        {
            if let Some(chunks_to_remove) = self.height_map.remove(&height) {
                for chunk_hash in chunks_to_remove {
                    if !requested_chunks.cache_get(&chunk_hash).is_some() {
                        self.encoded_chunks.remove(&chunk_hash);
                    }
                }
            }
        }
    }

    pub fn insert_chunk_header(&mut self, shard_id: ShardId, header: ShardChunkHeader) {
        if header.inner.height_created
            > self.largest_seen_height.saturating_sub(NUM_BLOCK_HASH_TO_CHUNK_HEADER as BlockIndex)
        {
            let mut block_hash_to_chunk_headers = self
                .block_hash_to_chunk_headers
                .cache_remove(&header.inner.prev_block_hash)
                .unwrap_or_else(|| vec![]);
            let prev_block_hash = header.inner.prev_block_hash;
            block_hash_to_chunk_headers.push((shard_id, header));
            self.block_hash_to_chunk_headers
                .cache_set(prev_block_hash, block_hash_to_chunk_headers);
        }
    }

    pub fn get_chunk_headers_for_block(
        &mut self,
        prev_block_hash: &CryptoHash,
    ) -> Vec<(ShardId, ShardChunkHeader)> {
        self.block_hash_to_chunk_headers.cache_remove(prev_block_hash).unwrap_or_else(|| vec![])
    }
}
