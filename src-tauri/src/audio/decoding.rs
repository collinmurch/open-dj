use crate::audio::config::DEFAULT_MONO_SAMPLE_CAPACITY;

use super::errors::AudioDecodingError;
use std::fs::File;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

/// Decodes an audio file to mono f32 samples.
pub(crate) fn decode_file_to_mono_samples(
    path: &str,
) -> Result<(Vec<f32>, f32), AudioDecodingError> {
    let file = File::open(path).map_err(|e| AudioDecodingError::FileOpenError {
        path: path.to_string(),
        source: e,
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AudioDecodingError::FormatError {
            path: path.to_string(),
            source: e,
        })?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| AudioDecodingError::NoSuitableTrack {
            path: path.to_string(),
        })?;
    let track_id = track.id;
    let sample_rate =
        track
            .codec_params
            .sample_rate
            .ok_or_else(|| AudioDecodingError::MissingSampleRate {
                path: path.to_string(),
            })? as f32;
    let channels = track
        .codec_params
        .channels
        .ok_or_else(|| AudioDecodingError::MissingChannelInfo {
            path: path.to_string(),
        })?
        .count();
    let codec_params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| AudioDecodingError::DecoderCreationError {
            path: path.to_string(),
            source: e,
        })?;
    // Estimate capacity based on typical audio file sizes for better memory allocation
    let estimated_duration_secs = 5.0 * 60.0; // Assume 5 minutes max for initial allocation
    let estimated_capacity = (estimated_duration_secs * sample_rate) as usize;
    let initial_capacity = estimated_capacity.min(DEFAULT_MONO_SAMPLE_CAPACITY * 4).max(DEFAULT_MONO_SAMPLE_CAPACITY);
    
    let mut samples: Vec<f32> = Vec::with_capacity(initial_capacity);
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    loop {
        match format.next_packet() {
            Ok(packet) => {
                if packet.track_id() != track_id {
                    continue;
                }
                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        if sample_buf.is_none() {
                            sample_buf = Some(SampleBuffer::<f32>::new(
                                audio_buf.capacity() as u64,
                                *audio_buf.spec(),
                            ));
                        }
                        if let Some(buf) = sample_buf.as_mut() {
                            buf.copy_interleaved_ref(audio_buf);
                            let raw_samples = buf.samples();
                            
                            // Optimized channel conversion with pre-allocation
                            if channels > 1 {
                                let mono_samples_count = raw_samples.len() / channels;
                                samples.reserve(mono_samples_count);
                                
                                let channel_div = 1.0 / channels as f32;
                                for chunk in raw_samples.chunks_exact(channels) {
                                    let sum: f32 = chunk.iter().sum();
                                    samples.push(sum * channel_div);
                                }
                            } else {
                                samples.extend_from_slice(raw_samples);
                            }
                        }
                    }
                    Err(SymphoniaError::DecodeError(err_desc)) => {
                        log::warn!(
                            "Central Decode: Ignoring decode error in '{}': {}",
                            path,
                            err_desc
                        );
                    }
                    Err(e) => {
                        return Err(AudioDecodingError::FatalDecodeError {
                            path: path.to_string(),
                            source: e,
                        });
                    }
                }
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                log::debug!("Central Decode: Reached EOF for '{}'", path);
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                log::warn!(
                    "Central Decode: Decoder reset required unexpectedly for '{}'",
                    path
                );
                break;
            }
            Err(e) => {
                return Err(AudioDecodingError::PacketReadIoError {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
    }
    decoder.finalize();
    log::debug!(
        "Central Decode: Decoded {} mono samples at {} Hz for '{}'",
        samples.len(),
        sample_rate,
        path
    );
    if samples.is_empty() {
        return Err(AudioDecodingError::NoSamplesDecoded {
            path: path.to_string(),
        });
    }
    Ok((samples, sample_rate))
}
