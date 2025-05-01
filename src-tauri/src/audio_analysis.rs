use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(serde::Serialize, Debug, Clone)]
pub struct VolumeInterval {
    start_time: f64,
    end_time: f64,
    rms_amplitude: f32,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct AudioAnalysis {
    intervals: Vec<VolumeInterval>,
    max_rms_amplitude: f32,
}

// Helper function to probe the file and find the primary audio track
fn probe_audio_track(
    path: &str,
) -> Result<(Box<dyn FormatReader + Send + Sync>, Track), String> {
    let src = File::open(path).map_err(|e| format!("Failed to open file '{}': {}", path, e))?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| format!("Unsupported format or error probing file: {}", e))?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .cloned() // Clone the track parameters
        .ok_or_else(|| "No suitable audio track found in the file.".to_string())?;

    log::debug!(
        "Found audio track #{}: Sample Rate={}, Channels={}",
        track.id,
        track.codec_params.sample_rate.unwrap_or(0),
        track.codec_params.channels.map(|c| c.count()).unwrap_or(0)
    );

    Ok((probed.format, track))
}

// Helper function to decode all samples from the chosen track
fn decode_track_samples(
    format: &mut dyn FormatReader,
    track_id: u32,
    channels: usize,
) -> Result<Vec<f32>, String> {
    let dec_opts: DecoderOptions = Default::default();
    // Find the track again within the format reader to get codec params for decoder creation
    let track = format
        .tracks()
        .iter()
        .find(|t| t.id == track_id)
        .ok_or("Track not found in format reader during decode setup.".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let mut all_samples: Vec<f32> = Vec::with_capacity(1024 * 256);
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
                            // Correctly iterate over samples slice in chunks
                            for frame in buf.samples().chunks_exact(channels) {
                                let mono_sample: f32 = frame.iter().sum::<f32>() / channels as f32;
                                all_samples.push(mono_sample);
                            }
                        }
                    }
                    Err(Error::DecodeError(err)) => log::warn!("Ignoring decode error: {}", err),
                    Err(e) => {
                        log::error!("Fatal decode error: {}", e);
                        return Err(format!("Decoder error: {}", e));
                    }
                }
            }
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => {
                log::warn!("Decoder reset required, stopping decoding.");
                break;
            }
            Err(e) => {
                log::error!("Error reading packet: {}", e);
                return Err(format!("Error reading audio packet: {}", e));
            }
        }
    }

    decoder.finalize();
    log::info!(
        "Finished decoding. Total mono samples: {}",
        all_samples.len()
    );
    Ok(all_samples)
}

// Helper function to calculate intervals and max RMS
fn calculate_rms_intervals(samples: &[f32], sample_rate: u32) -> (Vec<VolumeInterval>, f32) {
    if samples.is_empty() {
        return (Vec::new(), 0.0);
    }

    const TARGET_INTERVALS_PER_SECOND: f64 = 25.0;
    let samples_per_interval = ((sample_rate as f64) / TARGET_INTERVALS_PER_SECOND)
        .round()
        .max(1.0) as usize;

    let total_duration_seconds = samples.len() as f64 / sample_rate as f64;
    let num_intervals = (samples.len() as f64 / samples_per_interval as f64).ceil() as usize;

    let mut intervals: Vec<VolumeInterval> = Vec::with_capacity(num_intervals);
    let mut max_rms_amplitude: f32 = 0.0;

    for (i, chunk) in samples.chunks(samples_per_interval).enumerate() {
        let sum_sq: f64 = chunk.iter().map(|&s| (s as f64).powi(2)).sum();
        let mean_sq = sum_sq / chunk.len() as f64;
        let rms = mean_sq.sqrt().max(0.0) as f32;

        max_rms_amplitude = max_rms_amplitude.max(rms);

        let start_sample_index = i * samples_per_interval;
        let end_sample_index = start_sample_index + chunk.len(); // Use actual chunk len

        let start_time = start_sample_index as f64 / sample_rate as f64;
        let end_time =
            (end_sample_index as f64 / sample_rate as f64).min(total_duration_seconds);

        intervals.push(VolumeInterval {
            start_time,
            end_time,
            rms_amplitude: rms,
        });
    }

    // Ensure max_rms is non-zero if we have intervals, preventing division by zero later
    if max_rms_amplitude == 0.0 && !intervals.is_empty() {
        max_rms_amplitude = 0.0001;
    }

    log::info!(
        "Calculated RMS for {} intervals. Max RMS: {}",
        intervals.len(),
        max_rms_amplitude
    );

    (intervals, max_rms_amplitude)
}

#[tauri::command]
pub fn process_audio_file(path: String) -> Result<AudioAnalysis, String> {
    log::info!("Processing audio file: {}", path);

    // 1. Probe and find track
    let (mut format_reader, track) = probe_audio_track(&path)?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("Track sample rate is unknown.".to_string())?;
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    // 2. Decode samples
    let samples = decode_track_samples(&mut *format_reader, track.id, channels)?;

    // 3. Calculate analysis
    let (intervals, max_rms_amplitude) = calculate_rms_intervals(&samples, sample_rate);

    // 4. Combine results
    let analysis = AudioAnalysis {
        intervals,
        max_rms_amplitude,
    };

    Ok(analysis)
}
