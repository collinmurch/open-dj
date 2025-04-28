use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// Define a struct to hold the result for each interval
#[derive(serde::Serialize, Debug, Clone)]
pub struct VolumeInterval {
    start_time: f64,
    end_time: f64,
    rms_amplitude: f32,
}

#[tauri::command]
pub fn process_audio_file(path: String) -> Result<Vec<VolumeInterval>, String> {
	log::info!("Attempting to process audio file: {}", path);

	// Open the media source.
	let src = File::open(&path).map_err(|e| format!("Failed to open file '{}': {}", path, e))?;
	let mss = MediaSourceStream::new(Box::new(src), Default::default());

	// Create a hint to help the format probe identify the file format.
	let mut hint = Hint::new();
	if let Some(ext) = Path::new(&path).extension().and_then(|s| s.to_str()) {
		hint.with_extension(ext);
	}

	let meta_opts: MetadataOptions = Default::default();
	let fmt_opts: FormatOptions = Default::default();

	// Probe the format of the media source.
	let probed = symphonia::default::get_probe()
		.format(&hint, mss, &fmt_opts, &meta_opts)
		.map_err(|e| format!("Unsupported format or error probing file: {}", e))?;
	let mut format = probed.format;

	// Find the first audio track with a known codec.
	let track = format
		.tracks()
		.iter()
		.find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
		.ok_or_else(|| "No suitable audio track found in the file.".to_string())?;

	let sample_rate = track.codec_params.sample_rate.unwrap();
	let channels = track
		.codec_params
		.channels
		.map(|c| c.count())
		.unwrap_or(1);
	let track_id = track.id;
	log::debug!(
		"Found audio track #{}: Sample Rate={}, Channels={}",
		track_id,
		sample_rate,
		channels
	);

	// Create a decoder for the track.
	let dec_opts: DecoderOptions = Default::default();
	let mut decoder = symphonia::default::get_codecs()
		.make(&track.codec_params, &dec_opts)
		.map_err(|e| format!("Failed to create decoder: {}", e))?;

	let mut all_samples: Vec<f32> = Vec::new();
	let mut sample_buf: Option<SampleBuffer<f32>> = None;

	// Decode loop: Process packets until EOF or error.
	loop {
		match format.next_packet() {
			Ok(packet) => {
				// Process only packets from the selected audio track.
				if packet.track_id() != track_id {
					continue;
				}

				// Decode the packet into audio samples.
				match decoder.decode(&packet) {
					Ok(audio_buf) => {
						// Initialize the sample buffer if needed.
						if sample_buf.is_none() {
							sample_buf = Some(SampleBuffer::<f32>::new(
								audio_buf.capacity() as u64,
								*audio_buf.spec(),
							));
						}

						// Copy decoded samples and convert to mono by averaging channels.
						if let Some(buf) = sample_buf.as_mut() {
							buf.copy_interleaved_ref(audio_buf);
							for i in 0..buf.len() / channels {
								let frame_start = i * channels;
								let mono_sample: f32 = (0..channels)
									.map(|ch| buf.samples()[frame_start + ch])
									.sum::<f32>()
									/ channels as f32;
								all_samples.push(mono_sample);
							}
						}
					}
					Err(Error::DecodeError(err)) => {
						// Non-fatal decode errors can be logged and skipped.
						log::warn!("Ignoring decode error: {}", err);
					}
					Err(e) => {
						// Fatal decode errors stop processing.
						log::error!("Fatal decode error: {}", e);
						return Err(format!("Decoder error: {}", e));
					}
				}
			}
			Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // End of stream - normal termination
                log::debug!("Reached end of stream during packet reading.");
                break;
            }
			Err(Error::ResetRequired) => {
				// Rare case, log and break.
				log::warn!("Decoder reset required, stopping decoding.");
				break;
			}
			Err(e) => {
				// Other IO errors during packet reading are fatal.
				log::error!("Error reading packet: {}", e);
				return Err(format!("Error reading audio packet: {}", e));
			}
		}
	}

	// Finalize the decoder for any remaining buffered frames.
	decoder.finalize();
	log::info!("Finished decoding. Total mono samples: {}", all_samples.len());

	if all_samples.is_empty() {
		log::warn!("No samples decoded, returning empty results.");
		return Ok(Vec::new());
	}

	// Calculate RMS amplitude over smaller, fixed time intervals for waveform data.
	const TARGET_INTERVALS_PER_SECOND: f64 = 25.0; // Aim for 100 data points per second
	let samples_per_interval = ((sample_rate as f64) / TARGET_INTERVALS_PER_SECOND)
		.round()
		.max(1.0) as usize;

	let total_duration_seconds = all_samples.len() as f64 / sample_rate as f64;
	let num_intervals_precise = all_samples.len() as f64 / samples_per_interval as f64;
	let num_intervals = num_intervals_precise.ceil() as usize;
	let mut results: Vec<VolumeInterval> = Vec::with_capacity(num_intervals);

	log::debug!(
		"Calculating RMS for {} intervals of {} samples (~{}ms interval duration)",
		num_intervals,
		samples_per_interval,
		1000.0 / TARGET_INTERVALS_PER_SECOND
	);

	for i in (0..all_samples.len()).step_by(samples_per_interval) {
		let end_index = (i + samples_per_interval).min(all_samples.len());
		let chunk = &all_samples[i..end_index];

		if chunk.is_empty() {
			continue; // Should not happen with the loop logic, but safety first.
		}

		// Calculate RMS: sqrt(mean(sample^2))
		let sum_sq: f64 = chunk.iter().map(|&s| (s as f64).powi(2)).sum();
		let mean_sq = sum_sq / chunk.len() as f64;
		let rms = mean_sq.sqrt().max(0.0) as f32; // Ensure non-negative.

		let start_time = i as f64 / sample_rate as f64;
		// Ensure end_time doesn't exceed total duration, especially for the last chunk.
		let end_time = (end_index as f64 / sample_rate as f64).min(total_duration_seconds);

		results.push(VolumeInterval {
			start_time,
			end_time,
			rms_amplitude: rms,
		});
	}

	log::info!(
		"Successfully calculated RMS for {} granular intervals.",
		results.len()
	);
	Ok(results)
} 