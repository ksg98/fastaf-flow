use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};
use std::time::{Duration, Instant};

pub const MAX_SECONDS: u64 = 300;
pub struct Recording {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    error: Arc<Mutex<Option<String>>>,
    peak: Arc<AtomicU32>,
    sample_rate: u32,
    pub started: Instant,
    pub device: String,
}
impl Recording {
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No microphone found. Connect one and try again.")?;
        let supported = device.default_input_config().context("Cannot open microphone. Allow microphone access in System Settings → Privacy & Security.")?;
        let config: cpal::StreamConfig = supported.clone().into();
        let rate = config.sample_rate.0;
        let samples = Arc::new(Mutex::new(Vec::with_capacity(
            rate as usize * MAX_SECONDS as usize,
        )));
        let peak = Arc::new(AtomicU32::new(0));
        let error = Arc::new(Mutex::new(None));
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(
                &device,
                &config,
                samples.clone(),
                peak.clone(),
                error.clone(),
            ),
            cpal::SampleFormat::I16 => build::<i16>(
                &device,
                &config,
                samples.clone(),
                peak.clone(),
                error.clone(),
            ),
            cpal::SampleFormat::U16 => build::<u16>(
                &device,
                &config,
                samples.clone(),
                peak.clone(),
                error.clone(),
            ),
            cpal::SampleFormat::I32 => build::<i32>(
                &device,
                &config,
                samples.clone(),
                peak.clone(),
                error.clone(),
            ),
            cpal::SampleFormat::F64 => build::<f64>(
                &device,
                &config,
                samples.clone(),
                peak.clone(),
                error.clone(),
            ),
            format => bail!(
                "Microphone uses unsupported sample format {format:?}. Choose another default input device."
            ),
        }?;
        stream
            .play()
            .context("Cannot start recording. Check microphone permission.")?;
        Ok(Self {
            stream,
            samples,
            peak,
            sample_rate: rate,
            error,
            started: Instant::now(),
            device: device
                .name()
                .unwrap_or_else(|_| "Default microphone".into()),
        })
    }
    pub fn level(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }
    pub fn stop(self) -> Result<tempfile::NamedTempFile> {
        drop(self.stream);
        if let Some(error) = self.error.lock().unwrap().take() {
            bail!("Microphone disconnected: {error}");
        }
        let samples = self.samples.lock().unwrap();
        if samples.len() < (self.sample_rate / 10) as usize {
            bail!("No audio captured. Check microphone access and record a little longer.");
        }
        let file = tempfile::Builder::new()
            .prefix("fastaf-flow-")
            .suffix(".wav")
            .tempfile()?;
        let mut writer = hound::WavWriter::new(
            file.reopen()?,
            hound::WavSpec {
                channels: 1,
                sample_rate: self.sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for &sample in samples.iter() {
            writer.write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        writer.finalize()?;
        Ok(file)
    }
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    peak: Arc<AtomicU32>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let limit = config.sample_rate.0 as usize * MAX_SECONDS as usize;
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            if let Ok(mut buffer) = samples.try_lock() {
                let mut energy = 0.0_f32;
                let count = data.len() / channels;
                for frame in data.chunks_exact(channels) {
                    let sample = frame
                        .iter()
                        .map(|&s| <f32 as cpal::Sample>::from_sample(s))
                        .sum::<f32>()
                        / channels as f32;
                    let sample = if sample.is_finite() {
                        sample.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    };
                    energy += sample * sample;
                    if buffer.len() < limit {
                        buffer.push(sample);
                    }
                }
                peak.store(
                    (energy / count.max(1) as f32).sqrt().to_bits(),
                    Ordering::Relaxed,
                );
            }
        },
        move |err| {
            if let Ok(mut e) = error.lock() {
                *e = Some(err.to_string());
            }
        },
        None,
    )?)
}
