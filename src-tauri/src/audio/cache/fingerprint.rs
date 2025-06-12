use super::{AudioFingerprint, CacheError, CacheResult};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::SystemTime;

pub fn compute_content_hash(file_path: &Path) -> CacheResult<String> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = vec![0; 64 * 1024]; // 64KB buffer

    let bytes_read = reader.read(&mut buffer)?;
    buffer.truncate(bytes_read);

    let hash = blake3::hash(&buffer);
    Ok(hash.to_hex().to_string())
}

pub fn create_fingerprint(file_path: &str) -> CacheResult<AudioFingerprint> {
    let path = Path::new(file_path);

    // Get file metadata
    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();
    let last_modified = metadata.modified()?;

    // Compute content hash
    let content_hash = compute_content_hash(path)?;

    // Get audio metadata using existing decoding
    let (samples, sample_rate) = crate::audio::decoding::decode_file_to_mono_samples(file_path)
        .map_err(|e| {
            CacheError::EntryCorrupted(format!("Failed to decode audio for fingerprint: {}", e))
        })?;

    let duration_ms = if sample_rate > 0.0 && !samples.is_empty() {
        ((samples.len() as f64 / sample_rate as f64) * 1000.0) as u64
    } else {
        0
    };

    Ok(AudioFingerprint {
        content_hash,
        duration_ms,
        sample_rate: sample_rate as u32,
        file_size,
        last_modified,
    })
}

pub fn validate_cache_entry(
    file_path: &str,
    cached_fingerprint: &AudioFingerprint,
) -> CacheResult<bool> {
    let path = Path::new(file_path);

    // Quick file metadata check
    let metadata = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(_) => return Ok(false), // File doesn't exist
    };

    let file_size = metadata.len();
    let last_modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    // Check if file size or modification time changed
    if file_size != cached_fingerprint.file_size
        || last_modified != cached_fingerprint.last_modified
    {
        log::debug!("File metadata changed for: {}", file_path);
        return Ok(false);
    }

    // If metadata is the same, we can trust the cache
    // For extra validation, we could rehash, but that's expensive
    Ok(true)
}
