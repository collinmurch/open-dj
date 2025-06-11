pub mod init;
pub mod track;
pub mod playback;
pub mod audio_effects;

pub(crate) use init::*;
pub(crate) use track::*;
pub(crate) use playback::*;
pub(crate) use audio_effects::*;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use super::state::{AtomicF32, AtomicF64, AudioThreadDeckState};
use crate::audio::config::{INITIAL_TRIM_GAIN, EQ_RECALC_THRESHOLD_DB, EQ_SMOOTHING_FACTOR};
use crate::audio::decoding;
use crate::audio::effects;
use crate::audio::errors::PlaybackError;
use crate::audio::types::EqParams;

use super::events::*;
use biquad::{Biquad, DirectForm1};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, StreamConfig, SupportedStreamConfigRange};
use tauri::{AppHandle, Runtime};