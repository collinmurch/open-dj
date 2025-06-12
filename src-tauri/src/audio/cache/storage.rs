use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use super::{CachedTrackData, CacheResult, CacheError};

pub fn ensure_cache_directory(music_dir: &Path) -> CacheResult<PathBuf> {
    let cache_dir = music_dir.join(".open-dj").join("cache").join("metadata");
    
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir)
            .map_err(|e| CacheError::DirectoryCreation(format!("Failed to create cache directory: {}", e)))?;
        log::info!("Created cache directory: {}", cache_dir.display());
    }
    
    Ok(cache_dir)
}

pub fn load_cached_data(cache_dir: &Path, hash: &str) -> CacheResult<CachedTrackData> {
    let cache_file = cache_dir.join(format!("{}.json", hash));
    
    if !cache_file.exists() {
        return Err(CacheError::EntryNotFound(hash.to_string()));
    }
    
    let file = File::open(&cache_file)?;
    let reader = BufReader::new(file);
    
    let cached_data: CachedTrackData = serde_json::from_reader(reader)
        .map_err(|e| CacheError::EntryCorrupted(format!("Failed to deserialize cache file {}: {}", cache_file.display(), e)))?;
    
    Ok(cached_data)
}

pub fn save_cached_data(cache_dir: &Path, hash: &str, data: &CachedTrackData) -> CacheResult<()> {
    let cache_file = cache_dir.join(format!("{}.json", hash));
    let temp_file = cache_dir.join(format!("{}.json.tmp", hash));
    
    // Ensure cache directory exists
    if !cache_dir.exists() {
        fs::create_dir_all(cache_dir)?;
    }
    
    // Write to temporary file first (atomic operation)
    {
        let file = File::create(&temp_file)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, data)?;
    }
    
    // Atomic rename
    fs::rename(&temp_file, &cache_file)?;
    
    log::debug!("Cached analysis data for hash: {}", hash);
    Ok(())
}

pub fn delete_cached_data(cache_dir: &Path, hash: &str) -> CacheResult<()> {
    let cache_file = cache_dir.join(format!("{}.json", hash));
    
    if cache_file.exists() {
        fs::remove_file(&cache_file)?;
        log::debug!("Deleted cache file for hash: {}", hash);
    }
    
    Ok(())
}

pub fn list_cache_files(cache_dir: &Path) -> CacheResult<Vec<String>> {
    if !cache_dir.exists() {
        return Ok(Vec::new());
    }
    
    let mut hashes = Vec::new();
    
    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.ends_with(".json") && !file_name.ends_with(".tmp") {
                let hash = file_name.trim_end_matches(".json");
                hashes.push(hash.to_string());
            }
        }
    }
    
    Ok(hashes)
}

pub fn get_cache_size(cache_dir: &Path) -> CacheResult<u64> {
    if !cache_dir.exists() {
        return Ok(0);
    }
    
    let mut total_size = 0;
    
    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        total_size += metadata.len();
    }
    
    Ok(total_size)
}

pub fn cleanup_temp_files(cache_dir: &Path) -> CacheResult<()> {
    if !cache_dir.exists() {
        return Ok(());
    }
    
    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.ends_with(".tmp") {
                if let Err(e) = fs::remove_file(&path) {
                    log::warn!("Failed to remove temp file {}: {}", path.display(), e);
                } else {
                    log::debug!("Cleaned up temp file: {}", path.display());
                }
            }
        }
    }
    
    Ok(())
}