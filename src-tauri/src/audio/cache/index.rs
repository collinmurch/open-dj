use super::{CacheIndex, CacheResult};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

const INDEX_FILE_NAME: &str = "index.json";

pub fn load_index(cache_dir: &Path) -> CacheResult<CacheIndex> {
    let index_file = cache_dir.join(INDEX_FILE_NAME);

    if !index_file.exists() {
        log::debug!("Index file not found, creating new index");
        return Ok(CacheIndex::default());
    }

    let file = File::open(&index_file)?;
    let reader = BufReader::new(file);

    match serde_json::from_reader::<_, CacheIndex>(reader) {
        Ok(index) => {
            log::debug!("Loaded cache index with {} entries", index.entries.len());
            Ok(index)
        }
        Err(e) => {
            log::warn!("Cache index corrupted ({}), rebuilding...", e);
            rebuild_index(cache_dir)
        }
    }
}

pub fn save_index(cache_dir: &Path, index: &CacheIndex) -> CacheResult<()> {
    let index_file = cache_dir.join(INDEX_FILE_NAME);
    let temp_file = cache_dir.join(format!("{}.tmp", INDEX_FILE_NAME));

    // Ensure cache directory exists
    if !cache_dir.exists() {
        fs::create_dir_all(cache_dir)?;
    }

    // Write to temporary file first (atomic operation)
    {
        let file = File::create(&temp_file)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, index)?;
    }

    // Atomic rename
    fs::rename(&temp_file, &index_file)?;

    log::debug!("Saved cache index with {} entries", index.entries.len());
    Ok(())
}

pub fn rebuild_index(cache_dir: &Path) -> CacheResult<CacheIndex> {
    log::info!("Rebuilding cache index from cached files...");

    let index = CacheIndex::default();

    if !cache_dir.exists() {
        log::debug!("Cache directory doesn't exist, returning empty index");
        return Ok(index);
    }

    // Get all cache files
    let cache_hashes = super::storage::list_cache_files(cache_dir)?;

    for hash in cache_hashes {
        // Try to load the cached data to validate it
        match super::storage::load_cached_data(cache_dir, &hash) {
            Ok(_cached_data) => {
                // Extract the original file path from the cached data
                // We need to reverse-engineer this from the cache structure
                // Since we don't store the original path in the cached data,
                // we'll need to rely on the current file system state

                // For now, we'll skip adding to index if we can't determine the path
                // This will cause cache misses until files are re-analyzed
                log::debug!("Found valid cache file for hash: {}", hash);
            }
            Err(e) => {
                log::warn!("Invalid cache file for hash {}: {}. Removing.", hash, e);
                if let Err(remove_err) = super::storage::delete_cached_data(cache_dir, &hash) {
                    log::warn!("Failed to remove invalid cache file: {}", remove_err);
                }
            }
        }
    }

    // Save the rebuilt index
    save_index(cache_dir, &index)?;

    log::info!("Rebuilt cache index with {} entries", index.entries.len());
    Ok(index)
}

pub fn cleanup_orphaned_cache(cache_dir: &Path, current_files: &[PathBuf]) -> CacheResult<()> {
    let mut index = load_index(cache_dir)?;
    let mut removed_count = 0;

    // Create a set of current files for fast lookup
    let current_files_set: std::collections::HashSet<_> = current_files.iter().collect();

    // Find entries in index that no longer exist in the file system
    let mut to_remove = Vec::new();
    for (path, hash) in &index.entries {
        if !current_files_set.contains(&path) {
            to_remove.push((path.clone(), hash.clone()));
        }
    }

    // Remove orphaned entries
    for (path, hash) in to_remove {
        index.entries.remove(&path);

        // Also remove the cache file
        if let Err(e) = super::storage::delete_cached_data(cache_dir, &hash) {
            log::warn!("Failed to remove orphaned cache file {}: {}", hash, e);
        } else {
            removed_count += 1;
            log::debug!("Removed orphaned cache entry: {}", path.display());
        }
    }

    if removed_count > 0 {
        save_index(cache_dir, &index)?;
        log::info!("Cleaned up {} orphaned cache entries", removed_count);
    }

    Ok(())
}

pub fn get_cache_stats(cache_dir: &Path) -> CacheResult<(usize, u64)> {
    let index = load_index(cache_dir)?;
    let cache_size = super::storage::get_cache_size(cache_dir)?;

    Ok((index.entries.len(), cache_size))
}
