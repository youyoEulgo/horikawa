//! Core player implementation
//!
//! The Player struct orchestrates decoding, output, and playback control.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

use rubato::{FastFixedOut, PolynomialDegree, Resampler};
use thiserror::Error;

use crate::decoder::{AudioMetadata, Decoder};
use crate::output::{AudioOutput, SampleBuffer};
use crate::playlist::Playlist;

/// Converts planar samples back to interleaved format.
/// [[L0, L1, ...], [R0, R1, ...]] → [L0, R0, L1, R1, ...]
fn interleave(channels: &[Vec<f32>]) -> Vec<f32> {
    if channels.is_empty() || channels[0].is_empty() {
        return Vec::new();
    }
    let frames = channels[0].len();
    let num_ch = channels.len();
    let mut out = Vec::with_capacity(frames * num_ch);
    for f in 0..frames {
        for ch in channels {
            out.push(ch[f]);
        }
    }
    out
}

/// Errors that can occur during playback.
#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("Failed to open file: {0}")]
    FileOpen(String),

    #[error("Decode error: {0}")]
    Decode(String),

    #[error("Audio output error: {0}")]
    Output(String),

    #[error("No track loaded")]
    NoTrack,
}

/// Current playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Events emitted by the player for UI updates.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    TrackChanged {
        path: PathBuf,
    },
    StateChanged {
        state: PlaybackState,
    },
    PositionChanged {
        position: Duration,
        duration: Duration,
    },
    TrackEnded,
    Error {
        message: String,
    },
}

/// Shared playback state between main thread and decode thread.
struct PlaybackHandle {
    stop_flag: Arc<AtomicBool>,
    sample_buffer: Arc<SampleBuffer>,
    thread: Option<thread::JoinHandle<()>>,
    /// Number of frames (samples / channels) decoded so far
    frames_played: Arc<AtomicU64>,
    /// Sample rate of the source file
    sample_rate: u32,
    /// Total duration of the track
    duration: Option<Duration>,
    /// Flag set when track ends naturally (EOF reached)
    track_ended: Arc<AtomicBool>,
    /// Metadata extracted from the audio file
    metadata: AudioMetadata,
}

struct PlaybackInit {
    sample_buffer: Arc<SampleBuffer>,
    sample_rate: u32,
    duration: Option<Duration>,
    metadata: AudioMetadata,
}

type PlaybackThreadInit = (
    PlaybackInit,
    Decoder,
    Option<FastFixedOut<f32>>,
    AudioOutput,
);

/// Core audio player.
pub struct Player {
    state: Arc<RwLock<PlaybackState>>,
    current_track: Arc<RwLock<Option<PathBuf>>>,
    playlist: Arc<RwLock<Playlist>>,
    playback: Arc<RwLock<Option<PlaybackHandle>>>,
    /// Volume level (0.0 to 1.5), persisted across track changes
    volume: Arc<RwLock<f32>>,
}

impl Player {
    /// Creates a new Player instance.
    pub fn new() -> Result<Self, PlayerError> {
        Ok(Self {
            state: Arc::new(RwLock::new(PlaybackState::Stopped)),
            current_track: Arc::new(RwLock::new(None)),
            playlist: Arc::new(RwLock::new(Playlist::new())),
            playback: Arc::new(RwLock::new(None)),
            volume: Arc::new(RwLock::new(1.0)),
        })
    }

    /// Starts playback of the specified file.
    pub fn play(&self, path: PathBuf) -> Result<(), PlayerError> {
        // Stop any current playback
        self.stop()?;

        tracing::info!("Playing: {:?}", path);

        let handle = self.start_playback_thread(path.clone(), None, false)?;

        // Store playback handle
        {
            let mut playback = self.playback.write().unwrap();
            *playback = Some(handle);
        }

        // Update state
        {
            let mut track = self.current_track.write().unwrap();
            *track = Some(path);
        }

        {
            let mut state = self.state.write().unwrap();
            *state = PlaybackState::Playing;
        }

        Ok(())
    }

    fn start_playback_thread(
        &self,
        path: PathBuf,
        seek_position: Option<Duration>,
        start_paused: bool,
    ) -> Result<PlaybackHandle, PlayerError> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let frames_played = Arc::new(AtomicU64::new(0));
        let track_ended = Arc::new(AtomicBool::new(false));
        let state_clone = Arc::clone(&self.state);
        let stop_flag_clone = Arc::clone(&stop_flag);
        let frames_played_clone = Arc::clone(&frames_played);
        let track_ended_clone = Arc::clone(&track_ended);
        let volume = *self.volume.read().unwrap();
        let (init_tx, init_rx) = mpsc::sync_channel(1);

        let thread = thread::Builder::new()
            .name("horikawa-playback".to_string())
            .spawn(move || {
                let init = (|| -> Result<PlaybackThreadInit, PlayerError> {
                    let mut decoder =
                        Decoder::open(&path).map_err(|e| PlayerError::FileOpen(e.to_string()))?;

                    if let Some(position) = seek_position {
                        decoder
                            .seek(position.as_secs_f64())
                            .map_err(|e| PlayerError::Decode(e.to_string()))?;
                    }

                    let source_sample_rate = decoder.sample_rate();
                    let channels = decoder.channels() as u16;
                    let duration = decoder.duration().map(Duration::from_secs_f64);
                    let metadata = decoder.metadata();

                    if let Some(position) = seek_position {
                        let seek_frames = (position.as_secs_f64() * source_sample_rate as f64) as u64;
                        frames_played_clone.store(seek_frames, Ordering::Relaxed);
                    }

                    let (output, sample_buffer) = AudioOutput::new(source_sample_rate, channels)
                        .map_err(|e| PlayerError::Output(e.to_string()))?;
                    tracing::info!(
                        "Audio output created on playback thread {:?} ({})",
                        thread::current().id(),
                        thread::current().name().unwrap_or("unnamed")
                    );

                    sample_buffer.set_volume(volume);
                    if start_paused {
                        sample_buffer.set_paused(true);
                    }

                    let target_sample_rate = output.sample_rate();
                    output
                        .play()
                        .map_err(|e| PlayerError::Output(e.to_string()))?;

                    let resampler = if source_sample_rate != target_sample_rate {
                        tracing::info!(
                            "Resampling: {} Hz → {} Hz",
                            source_sample_rate,
                            target_sample_rate
                        );
                        Some(
                            FastFixedOut::<f32>::new(
                                target_sample_rate as f64 / source_sample_rate as f64,
                                2.0,
                                PolynomialDegree::Cubic,
                                1024,
                                channels as usize,
                            )
                            .map_err(|e| PlayerError::Output(format!("Failed to create resampler: {}", e)))?,
                        )
                    } else {
                        None
                    };

                    Ok((
                        PlaybackInit {
                            sample_buffer,
                            sample_rate: source_sample_rate,
                            duration,
                            metadata,
                        },
                        decoder,
                        resampler,
                        output,
                    ))
                })();

                let (init, decoder, resampler, output) = match init {
                    Ok(init) => init,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };

                let sample_buffer = Arc::clone(&init.sample_buffer);
                if init_tx.send(Ok(init)).is_err() {
                    return;
                }

                Self::decode_loop(
                    decoder,
                    sample_buffer,
                    stop_flag_clone,
                    state_clone,
                    resampler,
                    frames_played_clone,
                    track_ended_clone,
                );
                drop(output);
                tracing::info!(
                    "Audio output dropped on playback thread {:?} ({})",
                    thread::current().id(),
                    thread::current().name().unwrap_or("unnamed")
                );
            })
            .map_err(|e| PlayerError::Output(format!("Failed to spawn playback thread: {}", e)))?;

        let init = match init_rx.recv() {
            Ok(Ok(init)) => init,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(e) => {
                let _ = thread.join();
                return Err(PlayerError::Output(format!(
                    "Playback thread exited during initialization: {}",
                    e
                )));
            }
        };

        Ok(PlaybackHandle {
            stop_flag,
            sample_buffer: init.sample_buffer,
            thread: Some(thread),
            frames_played,
            sample_rate: init.sample_rate,
            duration: init.duration,
            track_ended,
            metadata: init.metadata,
        })
    }

    /// The decode loop that runs in a separate thread.
    fn decode_loop(
        mut decoder: Decoder,
        sample_buffer: Arc<SampleBuffer>,
        stop_flag: Arc<AtomicBool>,
        state: Arc<RwLock<PlaybackState>>,
        mut resampler: Option<FastFixedOut<f32>>,
        frames_played: Arc<AtomicU64>,
        track_ended: Arc<AtomicBool>,
    ) {
        let channels = decoder.channels();

        // Input buffer for resampler (stores planar samples per channel)
        let mut resample_input: Vec<Vec<f32>> = (0..channels).map(|_| Vec::new()).collect();

        loop {
            // Check for stop signal
            if stop_flag.load(Ordering::Relaxed) {
                tracing::debug!("Decode loop: stop signal received");
                break;
            }

            // Check for pause signal - if paused, just sleep
            if sample_buffer.is_paused() {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            // Check if output buffer has room
            // Don't decode too far ahead - keep about 50ms buffered
            let target_buffer = (decoder.sample_rate() as usize * channels) / 20;
            if sample_buffer.len() > target_buffer {
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            // Decode next chunk
            match decoder.decode_next() {
                Ok(Some(samples)) => {
                    // Track position based on source frames (before resampling)
                    let source_frames = samples.len() / channels;
                    frames_played.fetch_add(source_frames as u64, Ordering::Relaxed);

                    // Apply resampling if needed
                    let output_samples = if let Some(ref mut resampler) = resampler {
                        // Add new samples to input buffer (convert interleaved to planar)
                        for chunk in samples.chunks(channels) {
                            for (ch_idx, sample) in chunk.iter().enumerate() {
                                if ch_idx < resample_input.len() {
                                    resample_input[ch_idx].push(*sample);
                                }
                            }
                        }

                        // Process when we have enough input frames
                        let mut output_interleaved = Vec::new();
                        while resample_input[0].len() >= resampler.input_frames_next() {
                            let needed = resampler.input_frames_next();

                            // Extract needed frames from input buffer
                            let input_chunk: Vec<Vec<f32>> = resample_input
                                .iter_mut()
                                .map(|ch| ch.drain(..needed).collect())
                                .collect();

                            // Resample
                            match resampler.process(&input_chunk, None) {
                                Ok(resampled) => {
                                    output_interleaved.extend(interleave(&resampled));
                                }
                                Err(e) => {
                                    tracing::error!("Resample error: {}", e);
                                    // Put samples back on error
                                    for (ch_idx, samples) in input_chunk.into_iter().enumerate() {
                                        for sample in samples.into_iter().rev() {
                                            resample_input[ch_idx].insert(0, sample);
                                        }
                                    }
                                    break;
                                }
                            }
                        }

                        output_interleaved
                    } else {
                        samples
                    };

                    // Push samples to buffer
                    if !output_samples.is_empty() {
                        let mut offset = 0;
                        while offset < output_samples.len() && !stop_flag.load(Ordering::Relaxed) {
                            let pushed = sample_buffer.push(&output_samples[offset..]);
                            offset += pushed;
                            if pushed == 0 {
                                // Buffer full, wait a bit
                                thread::sleep(Duration::from_millis(5));
                            }
                        }
                    }
                }
                Ok(None) => {
                    // EOF - flush any remaining samples in resample buffer
                    if let Some(ref mut resampler) = resampler {
                        if !resample_input[0].is_empty() {
                            // Use process_partial for remaining samples
                            match resampler.process_partial(Some(&resample_input), None) {
                                Ok(resampled) => {
                                    let output_interleaved = interleave(&resampled);
                                    let mut offset = 0;
                                    while offset < output_interleaved.len()
                                        && !stop_flag.load(Ordering::Relaxed)
                                    {
                                        let pushed =
                                            sample_buffer.push(&output_interleaved[offset..]);
                                        offset += pushed;
                                        if pushed == 0 {
                                            thread::sleep(Duration::from_millis(5));
                                        }
                                    }
                                }
                                Err(e) => tracing::error!("Final resample error: {}", e),
                            }
                        }
                    }

                    // Wait for buffer to drain, then signal end
                    tracing::info!("Decode loop: reached end of file");
                    while !sample_buffer.is_empty() && !stop_flag.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(10));
                    }
                    // Signal that track ended naturally (not stopped by user)
                    track_ended.store(true, Ordering::Relaxed);
                    // Update state to stopped
                    {
                        let mut s = state.write().unwrap();
                        *s = PlaybackState::Stopped;
                    }
                    break;
                }
                Err(e) => {
                    tracing::error!("Decode error: {}", e);
                    break;
                }
            }
        }

        tracing::debug!("Decode loop: exiting");
    }

    /// Pauses playback.
    pub fn pause(&self) -> Result<(), PlayerError> {
        let playback = self.playback.read().unwrap();
        if let Some(ref handle) = *playback {
            handle.sample_buffer.set_paused(true);

            let mut state = self.state.write().unwrap();
            *state = PlaybackState::Paused;
            tracing::info!("Paused");
        }
        Ok(())
    }

    /// Resumes playback.
    pub fn resume(&self) -> Result<(), PlayerError> {
        let playback = self.playback.read().unwrap();
        if let Some(ref handle) = *playback {
            handle.sample_buffer.set_paused(false);

            let mut state = self.state.write().unwrap();
            *state = PlaybackState::Playing;
            tracing::info!("Resumed");
        }
        Ok(())
    }

    /// Stops playback.
    pub fn stop(&self) -> Result<(), PlayerError> {
        let mut playback = self.playback.write().unwrap();

        if let Some(mut handle) = playback.take() {
            // Signal stop
            handle.stop_flag.store(true, Ordering::Relaxed);
            handle.sample_buffer.clear();

            // Wait for thread to finish
            if let Some(thread) = handle.thread.take() {
                let _ = thread.join();
            }

            // AudioOutput is dropped here, which stops the cpal stream
            tracing::info!("Stopped");
        }

        // Update state
        {
            let mut state = self.state.write().unwrap();
            *state = PlaybackState::Stopped;
        }

        {
            let mut track = self.current_track.write().unwrap();
            *track = None;
        }

        Ok(())
    }

    /// Gets the current playback state.
    pub fn state(&self) -> PlaybackState {
        *self.state.read().unwrap()
    }

    /// Gets the current track path, if any.
    pub fn current_track(&self) -> Option<PathBuf> {
        self.current_track.read().unwrap().clone()
    }

    /// Gets a reference to the playlist.
    pub fn playlist(&self) -> Arc<RwLock<Playlist>> {
        Arc::clone(&self.playlist)
    }

    /// Gets the current playback position.
    pub fn position(&self) -> Duration {
        let playback = self.playback.read().unwrap();
        if let Some(ref handle) = *playback {
            let frames = handle.frames_played.load(Ordering::Relaxed);
            let seconds = frames as f64 / handle.sample_rate as f64;
            Duration::from_secs_f64(seconds)
        } else {
            Duration::ZERO
        }
    }

    /// Gets the total duration of the current track.
    pub fn duration(&self) -> Option<Duration> {
        let playback = self.playback.read().unwrap();
        playback.as_ref().and_then(|h| h.duration)
    }

    /// Gets the metadata of the current track.
    pub fn metadata(&self) -> Option<AudioMetadata> {
        let playback = self.playback.read().unwrap();
        playback.as_ref().map(|h| h.metadata.clone())
    }

    /// Gets the visualization data (RMS amplitudes for frequency bars).
    pub fn vis_data(&self) -> Option<[f32; crate::output::VIS_BARS]> {
        let playback = self.playback.read().unwrap();
        playback.as_ref().map(|h| h.sample_buffer.vis_data())
    }

    /// Gets legacy volume-meter style visualization data.
    pub fn vis_rms(&self) -> Option<[f32; crate::output::VIS_BARS]> {
        let playback = self.playback.read().unwrap();
        playback.as_ref().map(|h| h.sample_buffer.vis_rms())
    }

    /// Sets the volume level (0.0 = mute, 1.0 = normal, >1.0 = boost).
    pub fn set_volume(&self, volume: f32) {
        // Store volume for future tracks
        {
            let mut vol = self.volume.write().unwrap();
            *vol = volume;
        }
        // Apply to current playback if any
        let playback = self.playback.read().unwrap();
        if let Some(ref handle) = *playback {
            handle.sample_buffer.set_volume(volume);
        }
    }

    /// Gets the current volume level.
    pub fn volume(&self) -> f32 {
        *self.volume.read().unwrap()
    }

    /// Returns true if the current track ended naturally (EOF reached).
    /// This is reset when a new track starts playing.
    pub fn track_ended(&self) -> bool {
        let playback = self.playback.read().unwrap();
        playback
            .as_ref()
            .map(|h| h.track_ended.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Plays the next track in the playlist.
    /// Returns Ok(true) if a track was started, Ok(false) if no next track.
    pub fn play_next(&self) -> Result<bool, PlayerError> {
        let track = {
            let mut playlist = self.playlist.write().unwrap();
            playlist.next_track().cloned()
        };

        if let Some(path) = track {
            self.play(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Plays the previous track in the playlist.
    /// Returns Ok(true) if a track was started, Ok(false) if no previous track.
    pub fn play_previous(&self) -> Result<bool, PlayerError> {
        let prev_track = {
            let mut playlist = self.playlist.write().unwrap();
            playlist.previous().cloned()
        };

        if let Some(path) = prev_track {
            self.play(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Seeks to a specific position in the current track.
    ///
    /// This works by stopping playback, reopening the file at the seek position,
    /// and resuming playback.
    pub fn seek(&self, position: Duration) -> Result<(), PlayerError> {
        let current_track = self.current_track().ok_or(PlayerError::NoTrack)?;
        let was_playing = self.state() == PlaybackState::Playing;

        // Stop current playback
        self.stop()?;

        // Reopen and seek
        tracing::info!("Seeking to {:?} in {:?}", position, current_track);

        let handle = self.start_playback_thread(current_track.clone(), Some(position), !was_playing)?;

        // Store playback handle
        {
            let mut playback = self.playback.write().unwrap();
            *playback = Some(handle);
        }

        // Update state
        {
            let mut track = self.current_track.write().unwrap();
            *track = Some(current_track);
        }

        {
            let mut state = self.state.write().unwrap();
            *state = if was_playing {
                PlaybackState::Playing
            } else {
                PlaybackState::Paused
            };
        }

        Ok(())
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new().expect("Failed to create player")
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Ensure playback is stopped when player is dropped
        let _ = self.stop();
    }
}
