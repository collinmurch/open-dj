use std::path::PathBuf;
use super::{storage, index};

#[tauri::command(async)]
pub fn ensure_cache_directory(music_dir: String) -> Result<String, String> {
    let music_path = PathBuf::from(music_dir);
    
    match storage::ensure_cache_directory(&music_path) {
        Ok(cache_dir) => Ok(cache_dir.to_string_lossy().to_string()),
        Err(e) => {
            log::warn!("Failed to create cache directory: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command(async)]
pub fn get_cache_stats(cache_dir: String) -> Result<(usize, u64), String> {
    let cache_path = PathBuf::from(cache_dir);
    
    match index::get_cache_stats(&cache_path) {
        Ok((entries, size)) => {
            log::info!("Cache stats: {} entries, {} bytes", entries, size);
            Ok((entries, size))
        }
        Err(e) => {
            log::warn!("Failed to get cache stats: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command(async)]
pub fn cleanup_cache(cache_dir: String, current_files: Vec<String>) -> Result<(), String> {
    let cache_path = PathBuf::from(cache_dir);
    let file_paths: Vec<PathBuf> = current_files.into_iter().map(PathBuf::from).collect();
    
    match index::cleanup_orphaned_cache(&cache_path, &file_paths) {
        Ok(()) => {
            log::info!("Cache cleanup completed successfully");
            Ok(())
        }
        Err(e) => {
            log::warn!("Cache cleanup failed: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command(async)]
pub fn rebuild_cache_index(cache_dir: String) -> Result<usize, String> {
    let cache_path = PathBuf::from(cache_dir);
    
    match index::rebuild_index(&cache_path) {
        Ok(rebuilt_index) => {
            let entry_count = rebuilt_index.entries.len();
            log::info!("Cache index rebuilt with {} entries", entry_count);
            Ok(entry_count)
        }
        Err(e) => {
            log::warn!("Failed to rebuild cache index: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command(async)]
pub fn clear_cache(cache_dir: String) -> Result<(), String> {
    let cache_path = PathBuf::from(cache_dir);
    
    // First, clean up temp files
    if let Err(e) = storage::cleanup_temp_files(&cache_path) {
        log::warn!("Failed to cleanup temp files during cache clear: {}", e);
    }
    
    // Remove all cache files
    match storage::list_cache_files(&cache_path) {
        Ok(hashes) => {
            for hash in hashes {
                if let Err(e) = storage::delete_cached_data(&cache_path, &hash) {
                    log::warn!("Failed to delete cache file {}: {}", hash, e);
                }
            }
            
            // Create a new empty index
            let empty_index = super::CacheIndex::default();
            match index::save_index(&cache_path, &empty_index) {
                Ok(()) => {
                    log::info!("Cache cleared successfully");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("Failed to save empty index after cache clear: {}", e);
                    Err(e.to_string())
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to list cache files during clear: {}", e);
            Err(e.to_string())
        }
    }
}