use crate::audio::types::{AudioAnalysis, TrackBasicMetadata};
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
    pub waveform_analysis: Option<AudioAnalysis>,
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

pub fn analyze_with_cache(
    file_path: &str,
    cache_dir: Option<&PathBuf>,
    include_waveform: bool,
) -> Result<(TrackBasicMetadata, Option<AudioAnalysis>), Box<dyn std::error::Error>> {
    if let Some(cache_dir) = cache_dir {
        // Try cache first
        match try_cache_lookup(file_path, cache_dir, include_waveform) {
            Ok(Some((metadata, waveform))) => {
                log::info!("Cache hit for: {}", file_path);
                return Ok((metadata, waveform));
            }
            Ok(None) => {
                log::debug!("Cache miss for: {}", file_path);
            }
            Err(e) => {
                log::warn!(
                    "Cache lookup failed for {}: {}. Proceeding without cache.",
                    file_path,
                    e
                );
            }
        }
    }

    // Fallback to regular analysis
    let (metadata, waveform) = if include_waveform {
        match crate::audio::processor::get_track_complete_analysis_internal(file_path) {
            Ok((meta, wave)) => (meta, Some(wave)),
            Err(e) => return Err(Box::new(e)),
        }
    } else {
        match crate::audio::processor::get_track_basic_metadata_internal(file_path) {
            Ok(meta) => (meta, None),
            Err(e) => return Err(Box::new(e)),
        }
    };

    // Cache the result if caching is enabled
    if let Some(cache_dir) = cache_dir {
        if let Err(e) = cache_analysis_result(file_path, cache_dir, &metadata, waveform.as_ref()) {
            log::warn!("Failed to cache result for {}: {}", file_path, e);
        }
    }

    Ok((metadata, waveform))
}

fn try_cache_lookup(
    file_path: &str,
    cache_dir: &PathBuf,
    include_waveform: bool,
) -> CacheResult<Option<(TrackBasicMetadata, Option<AudioAnalysis>)>> {
    // Load index
    let index = index::load_index(cache_dir)?;

    // Check if we have a cache entry
    let path_buf = PathBuf::from(file_path);
    if let Some(cached_hash) = index.entries.get(&path_buf) {
        // Load cached data
        if let Ok(cached_data) = storage::load_cached_data(cache_dir, cached_hash) {
            // Validate cache entry
            if fingerprint::validate_cache_entry(file_path, &cached_data.fingerprint)? {
                // Check if we have the required data
                if !include_waveform || cached_data.waveform_analysis.is_some() {
                    let waveform = if include_waveform {
                        cached_data.waveform_analysis.clone()
                    } else {
                        None
                    };
                    return Ok(Some((cached_data.bpm_analysis, waveform)));
                }
            } else {
                log::debug!("Cache entry invalid for: {}", file_path);
            }
        }
    }

    Ok(None)
}

fn cache_analysis_result(
    file_path: &str,
    cache_dir: &PathBuf,
    metadata: &TrackBasicMetadata,
    waveform: Option<&AudioAnalysis>,
) -> CacheResult<()> {
    // Create fingerprint
    let fingerprint = fingerprint::create_fingerprint(file_path)?;

    // Create cached data
    let cached_data = CachedTrackData {
        fingerprint: fingerprint.clone(),
        bpm_analysis: metadata.clone(),
        waveform_analysis: waveform.cloned(),
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
