//! Audio output via cpal
//!
//! Handles sending decoded PCM samples to the system audio device.

use std::sync::{ Arc, Mutex };
use std::sync::atomic::{ AtomicBool, AtomicU32, Ordering };
use std::collections::VecDeque;

use cpal::traits::{ DeviceTrait, HostTrait, StreamTrait };
use thiserror::Error;

use rustfft::{ FftPlanner, num_complex::Complex, FftDirection };


/// Errors that can occur with audio output.
#[derive( Debug, Error )]
pub enum OutputError {
    #[error( "No output device available" )]
    NoDevice,

    #[error( "Failed to get default stream config: {0}" )]
    StreamConfig( String ),

    #[error( "Failed to build output stream: {0}" )]
    BuildStream( String ),

    #[error( "Failed to play stream: {0}" )]
    PlayStream( String ),
}


/// Number of visualization bars to display
pub const VIS_BARS: usize = 64;

/// FFT size for spectrum analysis (power of 2).
const FFT_SIZE: usize = 1024;


/// Precomputed Hann window coefficients (lazy static after first call).
fn hann_window() -> &'static [f32; FFT_SIZE] {
    use std::sync::OnceLock;
    static WINDOW: OnceLock<[f32; FFT_SIZE]> = OnceLock::new();
    WINDOW.get_or_init(|| {
        let mut w = [0.0f32; FFT_SIZE];
        let n = FFT_SIZE as f32 - 1.0;
        for i in 0..FFT_SIZE {
            w[i] = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n).cos());
        }
        w
    })
}


/// Shared sample buffer between producer (decoder) and consumer (audio callback).
/// This is Send + Sync and can be shared across threads.
/// Handles channel conversion between source and output.
pub struct SampleBuffer {
    buffer: Mutex<VecDeque<f32>>,
    capacity: usize,
    paused: AtomicBool,
    /// Volume level stored as f32 bits (0.0 to 1.0+)
    volume: AtomicU32,
    source_channels: u16,
    output_channels: u16,
    /// Visualization data — FFT-based log-spaced spectrum
    vis_data: Mutex<[f32; VIS_BARS]>,
    /// Visualization data — RMS time-sliced (legacy volume-meter style)
    vis_rms: Mutex<[f32; VIS_BARS]>,
    /// FFT plan (lazy-init)
    fft_plan: Mutex<Option<std::sync::Arc<dyn rustfft::Fft<f32>>>>,
    /// FFT working buffer
    fft_buf: Mutex<Vec<Complex<f32>>>,
}


impl SampleBuffer {
    /// Creates a new sample buffer with the given capacity and channel configuration.
    ///
    /// - `capacity`: Maximum number of samples to buffer
    /// - `source_channels`: Number of channels in the source audio (from decoder)
    /// - `output_channels`: Number of channels expected by the output device
    pub fn new( capacity: usize, source_channels: u16, output_channels: u16 ) -> Self {
        Self {
            buffer: Mutex::new( VecDeque::with_capacity( capacity ) ),
            vis_data: Mutex::new( [0.0; VIS_BARS] ),
            vis_rms: Mutex::new( [0.0; VIS_BARS] ),
            capacity,
            paused: AtomicBool::new( false ),
            volume: AtomicU32::new( 1.0_f32.to_bits() ),
            source_channels,
            output_channels,
            fft_plan: Mutex::new( None ),
            fft_buf: Mutex::new( Vec::with_capacity( FFT_SIZE ) ),
        }
    }


    /// Pushes samples to the buffer. Returns number of samples actually pushed.
    /// Also updates visualization data via FFT spectrum analysis.
    pub fn push( &self, samples: &[f32] ) -> usize {
        let mut buf = self.buffer.lock().unwrap();
        let available = self.capacity.saturating_sub( buf.len() );
        let to_push = samples.len().min( available );
        buf.extend( samples[ ..to_push ].iter().copied() );
        drop( buf );

        // Accumulate samples for FFT and update RMS visualization
        if !samples.is_empty() {
            let mut fft_buf = self.fft_buf.lock().unwrap();
            for &s in &samples[..to_push] {
                fft_buf.push( Complex::new( s, 0.0 ) );
                if fft_buf.len() >= FFT_SIZE {
                    self.compute_fft( &mut *fft_buf );
                    fft_buf.clear();
                }
            }
            drop( fft_buf );
            self.compute_rms( &samples[..to_push] );
        }

        to_push
    }


    /// Computes FFT-based spectrum and bins results into VIS_BARS.
    fn compute_fft( &self, buf: &mut Vec<Complex<f32>> ) {
        // Ensure we have exactly FFT_SIZE points
        if buf.len() < FFT_SIZE {
            return;
        }
        buf.truncate( FFT_SIZE );

        // Apply Hann window
        let window = hann_window();
        for ( i, sample ) in buf.iter_mut().enumerate() {
            sample.im = 0.0;
            sample.re *= window[ i ];
        }

        // Lazy-init FFT plan
        let mut plan_guard = self.fft_plan.lock().unwrap();
        if plan_guard.is_none() {
            let mut planner = FftPlanner::<f32>::new();
            let plan = planner.plan_fft( FFT_SIZE, FftDirection::Forward );
            tracing::info!( "Initialized {} point FFT for visualizer", FFT_SIZE );
            *plan_guard = Some( plan );
        }
        drop( plan_guard );

        // Run FFT (in-place on buf)
        if let Some( ref plan ) = *self.fft_plan.lock().unwrap() {
            plan.process( &mut buf[..] );
        }

        // Compute magnitude spectrum (first N/2 bins — the rest is mirror)
        let num_bins = FFT_SIZE / 2;

        // Bin into VIS_BARS via log-spaced frequency mapping.
        // Freq bin k maps to frequency k * sample_rate / FFT_SIZE.
        // We map k → bar via log10(1 + k) scaled to [0, VIS_BARS).
        let mut vis = self.vis_data.lock().unwrap();

        // Temporary: accumulate magnitude per bar
        let mut bar_sum = [0.0f32; VIS_BARS];
        let mut bar_count = [0u32; VIS_BARS];

        for k in 0..num_bins {
            // Log-spaced bar index: log10(1 + 9*k/(num_bins-1)) → [0, 1)
            let frac = k as f32 / (num_bins - 1).max( 1 ) as f32;
            let log_frac = ( 1.0 + 9.0 * frac ).log10();
            let bar = ( log_frac * VIS_BARS as f32 ) as usize;
            let bar = bar.min( VIS_BARS - 1 );

            let mag = ( buf[ k ].re * buf[ k ].re + buf[ k ].im * buf[ k ].im ).sqrt();
            bar_sum[ bar ] += mag;
            bar_count[ bar ] += 1;
        }

        for b in 0..VIS_BARS {
            if bar_count[ b ] > 0 {
                let avg = bar_sum[ b ] / bar_count[ b ] as f32;
                // Scale — FFT gives raw magnitudes proportional to amplitude*N/2
                let scaled = ( avg / ( FFT_SIZE as f32 * 0.15 ) ).min( 1.0 );
                // Smooth with previous (decay)
                vis[ b ] = vis[ b ] * 0.75 + scaled * 0.25;
            } else {
                vis[ b ] *= 0.90; // natural decay for empty bands
            }
        }
    }


    /// Updates legacy RMS volume-meter visualization from raw samples.
    fn compute_rms( &self, samples: &[f32] ) {
        if samples.len() < VIS_BARS { return; }
        let mut rms = self.vis_rms.lock().unwrap();
        let per_bar = samples.len() / VIS_BARS;
        for ( b, bar ) in rms.iter_mut().enumerate() {
            let start = b * per_bar;
            let end = ( start + per_bar ).min( samples.len() );
            let sum_sq: f32 = samples[ start..end ].iter().map( |s| s * s ).sum();
            let val = ( sum_sq / ( end - start ) as f32 ).sqrt();
            *bar = *bar * 0.7 + val * 0.3;
        }
    }


    /// Gets the current FFT-based log-spaced spectrum data.
    pub fn vis_data( &self ) -> [f32; VIS_BARS] {
        *self.vis_data.lock().unwrap()
    }


    /// Gets the legacy RMS volume-meter style data.
    pub fn vis_rms( &self ) -> [f32; VIS_BARS] {
        *self.vis_rms.lock().unwrap()
    }


    /// Pops samples from the buffer into the output slice, handling channel conversion.
    /// Returns the number of output samples actually written.
    pub fn pop( &self, output: &mut [f32] ) -> usize {
        // If paused, output silence
        if self.paused.load( Ordering::Relaxed ) {
            for sample in output.iter_mut() {
                *sample = 0.0;
            }
            return 0;
        }

        let volume = f32::from_bits( self.volume.load( Ordering::Relaxed ) );
        let mut buf = self.buffer.lock().unwrap();
        let src_ch = self.source_channels as usize;
        let out_ch = self.output_channels as usize;

        let written = if src_ch == out_ch {
            // No conversion needed
            let to_pop = output.len().min( buf.len() );
            for i in 0..to_pop {
                output[ i ] = buf.pop_front().unwrap();
            }
            // Fill remaining with silence
            for i in to_pop..output.len() {
                output[ i ] = 0.0;
            }
            to_pop
        } else if src_ch == 1 && out_ch == 2 {
            // Mono to stereo: duplicate each sample
            let output_frames = output.len() / out_ch;
            let available_frames = buf.len() / src_ch;
            let frames_to_process = output_frames.min( available_frames );

            for i in 0..frames_to_process {
                let sample = buf.pop_front().unwrap();
                output[ i * 2 ] = sample;
                output[ i * 2 + 1 ] = sample;
            }
            // Fill remaining with silence
            for i in ( frames_to_process * out_ch )..output.len() {
                output[ i ] = 0.0;
            }
            frames_to_process * out_ch
        } else if src_ch == 2 && out_ch == 1 {
            // Stereo to mono: mix down
            let output_frames = output.len() / out_ch;
            let available_frames = buf.len() / src_ch;
            let frames_to_process = output_frames.min( available_frames );

            for i in 0..frames_to_process {
                let left = buf.pop_front().unwrap();
                let right = buf.pop_front().unwrap();
                output[ i ] = ( left + right ) * 0.5;
            }
            // Fill remaining with silence
            for i in frames_to_process..output.len() {
                output[ i ] = 0.0;
            }
            frames_to_process
        } else {
            // General case: simple remix (duplicate first channel or mix all to fewer)
            let output_frames = output.len() / out_ch;
            let available_frames = buf.len() / src_ch;
            let frames_to_process = output_frames.min( available_frames );

            for frame in 0..frames_to_process {
                // Read source frame
                let mut src_samples = Vec::with_capacity( src_ch );
                for _ in 0..src_ch {
                    src_samples.push( buf.pop_front().unwrap() );
                }

                // Write output frame
                for ch in 0..out_ch {
                    if ch < src_ch {
                        output[ frame * out_ch + ch ] = src_samples[ ch ];
                    } else {
                        // Duplicate last channel if output has more channels
                        output[ frame * out_ch + ch ] = src_samples[ src_ch - 1 ];
                    }
                }
            }
            // Fill remaining with silence
            for i in ( frames_to_process * out_ch )..output.len() {
                output[ i ] = 0.0;
            }
            frames_to_process * out_ch
        };

        // Apply volume to all output samples
        if volume != 1.0 {
            for sample in output[ ..written ].iter_mut() {
                *sample *= volume;
            }
        }

        written
    }


    /// Returns the number of samples currently in the buffer.
    pub fn len( &self ) -> usize {
        self.buffer.lock().unwrap().len()
    }


    /// Returns true if the buffer is empty.
    pub fn is_empty( &self ) -> bool {
        self.buffer.lock().unwrap().is_empty()
    }


    /// Clears the buffer.
    pub fn clear( &self ) {
        self.buffer.lock().unwrap().clear();
    }


    /// Sets paused state.
    pub fn set_paused( &self, paused: bool ) {
        self.paused.store( paused, Ordering::Relaxed );
    }


    /// Gets paused state.
    pub fn is_paused( &self ) -> bool {
        self.paused.load( Ordering::Relaxed )
    }


    /// Sets the volume level (0.0 = mute, 1.0 = normal, >1.0 = boost).
    pub fn set_volume( &self, volume: f32 ) {
        self.volume.store( volume.to_bits(), Ordering::Relaxed );
    }


    /// Gets the current volume level.
    pub fn volume( &self ) -> f32 {
        f32::from_bits( self.volume.load( Ordering::Relaxed ) )
    }
}


/// Audio output handler.
/// Note: This struct is NOT Send/Sync due to cpal::Stream.
/// Keep it on the thread where it was created.
pub struct AudioOutput {
    stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
}


impl AudioOutput {
    /// Creates a new audio output with the specified source sample rate and channels.
    ///
    /// Returns both the AudioOutput and a shared SampleBuffer that the caller should
    /// use to push decoded samples. The buffer handles channel conversion if needed.
    pub fn new(
        source_sample_rate: u32,
        source_channels: u16,
    ) -> Result<( Self, Arc<SampleBuffer> ), OutputError> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .ok_or( OutputError::NoDevice )?;

        tracing::info!( "Using output device: {:?}", device.name() );

        // Try to get a config matching our requirements
        // Priority: 1) exact match, 2) same sample rate any channels, 3) default with warning
        let supported_configs: Vec<_> = device
            .supported_output_configs()
            .map_err( |e| OutputError::StreamConfig( e.to_string() ) )?
            .collect();

        // First try: exact match (channels + sample rate)
        let config = if let Some( supported_config ) = supported_configs.iter().find( |c| {
            c.channels() == source_channels
                && c.min_sample_rate().0 <= source_sample_rate
                && c.max_sample_rate().0 >= source_sample_rate
        }) {
            supported_config.clone()
                .with_sample_rate( cpal::SampleRate( source_sample_rate ) )
                .config()
        }
        // Second try: any config that supports our sample rate (we'll handle channel conversion)
        else if let Some( supported_config ) = supported_configs.iter().find( |c| {
            c.min_sample_rate().0 <= source_sample_rate
                && c.max_sample_rate().0 >= source_sample_rate
        }) {
            tracing::info!(
                "Channel conversion: file has {} channels, device using {} channels",
                source_channels,
                supported_config.channels()
            );
            supported_config.clone()
                .with_sample_rate( cpal::SampleRate( source_sample_rate ) )
                .config()
        }
        // Last resort: default config (may have wrong sample rate!)
        else {
            let default_config = device
                .default_output_config()
                .map_err( |e| OutputError::StreamConfig( e.to_string() ) )?;
            tracing::warn!(
                "Sample rate mismatch: file is {} Hz, device defaulting to {} Hz - playback speed may be incorrect!",
                source_sample_rate,
                default_config.sample_rate().0
            );
            default_config.config()
        };

        tracing::info!(
            "Audio output config: {} Hz, {} channels",
            config.sample_rate.0,
            config.channels
        );

        // Create shared sample buffer with channel conversion info
        // Buffer size: ~500ms of audio
        let buffer_capacity = ( source_sample_rate as usize ) * ( source_channels as usize ) / 2;
        let sample_buffer = Arc::new( SampleBuffer::new(
            buffer_capacity,
            source_channels,
            config.channels,
        ));
        let sample_buffer_clone = Arc::clone( &sample_buffer );

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    sample_buffer_clone.pop( data );
                },
                |err| {
                    tracing::error!( "Audio output error: {}", err );
                },
                None,
            )
            .map_err( |e| OutputError::BuildStream( e.to_string() ) )?;

        Ok((
            Self {
                stream,
                sample_rate: config.sample_rate.0,
                channels: config.channels,
            },
            sample_buffer,
        ))
    }


    /// Starts audio output.
    pub fn play( &self ) -> Result<(), OutputError> {
        self.stream
            .play()
            .map_err( |e| OutputError::PlayStream( e.to_string() ) )
    }


    /// Pauses the audio stream.
    pub fn pause( &self ) -> Result<(), OutputError> {
        self.stream
            .pause()
            .map_err( |e| OutputError::PlayStream( e.to_string() ) )
    }


    /// Gets the actual sample rate.
    pub fn sample_rate( &self ) -> u32 {
        self.sample_rate
    }


    /// Gets the actual number of channels.
    pub fn channels( &self ) -> u16 {
        self.channels
    }
}
