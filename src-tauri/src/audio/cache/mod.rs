use crate::audio::types::TrackBasicMetadata;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

pub mod commands;
pub mod fingerprint;
pub mod index;
pub mod storage;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFingerprint {
    pub content_hash: String,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub file_size: u64,
    pub last_modified: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedTrackData {
    pub fingerprint: AudioFingerprint,
    pub bpm_analysis: TrackBasicMetadata,
    pub cached_at: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheIndex {
    pub version: u32,
    pub entries: HashMap<PathBuf, String>, // path -> content_hash
}

impl Default for CacheIndex {
    fn default() -> Self {
        Self {
            version: 1,
            entries: HashMap::new(),
        }
    }
}

pub type CacheResult<T> = Result<T, CacheError>;

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Cache directory creation failed: {0}")]
    DirectoryCreation(String),

    #[error("Cache entry not found for hash: {0}")]
    EntryNotFound(String),

    #[error("Cache entry invalid or corrupted: {0}")]
    EntryCorrupted(String),
}

pub fn analyze_bpm_with_cache(
    file_path: &str,
    cache_dir: Option<&PathBuf>,
) -> Result<TrackBasicMetadata, Box<dyn std::error::Error>> {
    if let Some(cache_dir) = cache_dir {
        // Try cache first
        match try_bpm_cache_lookup(file_path, cache_dir) {
            Ok(Some(metadata)) => {
                log::info!("BPM cache hit for: {}", file_path);
                return Ok(metadata);
            }
            Ok(None) => {
                log::debug!("BPM cache miss for: {}", file_path);
            }
            Err(e) => {
                log::warn!(
                    "BPM cache lookup failed for {}: {}. Proceeding without cache.",
                    file_path,
                    e
                );
            }
        }
    }

    // Fallback to regular BPM analysis
    let metadata = match crate::audio::processor::get_track_basic_metadata_internal(file_path) {
        Ok(meta) => meta,
        Err(e) => return Err(Box::new(e)),
    };

    // Cache the BPM result if caching is enabled
    if let Some(cache_dir) = cache_dir {
        if let Err(e) = cache_bpm_result(file_path, cache_dir, &metadata) {
            log::warn!("Failed to cache BPM result for {}: {}", file_path, e);
        }
    }

    Ok(metadata)
}

fn try_bpm_cache_lookup(
    file_path: &str,
    cache_dir: &PathBuf,
) -> CacheResult<Option<TrackBasicMetadata>> {
    // Load index
    let index = index::load_index(cache_dir)?;

    // Check if we have a cache entry
    let path_buf = PathBuf::from(file_path);
    if let Some(cached_hash) = index.entries.get(&path_buf) {
        // Load cached data
        if let Ok(cached_data) = storage::load_cached_data(cache_dir, cached_hash) {
            // Validate cache entry
            if fingerprint::validate_cache_entry(file_path, &cached_data.fingerprint)? {
                return Ok(Some(cached_data.bpm_analysis));
            } else {
                log::debug!("BPM cache entry invalid for: {}", file_path);
            }
        }
    }

    Ok(None)
}

fn cache_bpm_result(
    file_path: &str,
    cache_dir: &PathBuf,
    metadata: &TrackBasicMetadata,
) -> CacheResult<()> {
    // Create fingerprint
    let fingerprint = fingerprint::create_fingerprint(file_path)?;

    // Create cached data
    let cached_data = CachedTrackData {
        fingerprint: fingerprint.clone(),
        bpm_analysis: metadata.clone(),
        cached_at: SystemTime::now(),
    };

    // Save to cache
    storage::save_cached_data(cache_dir, &fingerprint.content_hash, &cached_data)?;

    // Update index
    let mut index = index::load_index(cache_dir).unwrap_or_default();
    index
        .entries
        .insert(PathBuf::from(file_path), fingerprint.content_hash);
    index::save_index(cache_dir, &index)?;

    Ok(())
}
