use std::io::Cursor;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::time::Duration;

use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, Trim};
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

const OPUS_RATE: i32 = 48_000;
pub const PEAKS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peaks(pub [u8; PEAKS]);

pub fn peaks(pcm: &Pcm) -> Peaks {
    let channels = usize::from(pcm.channels.max(1));
    let frames = pcm.samples.len() / channels;
    let mut levels = [0u8; PEAKS];
    if frames == 0 {
        return Peaks(levels);
    }
    for (bucket, level) in levels.iter_mut().enumerate() {
        let start = bucket * frames / PEAKS * channels;
        let end = ((bucket + 1) * frames / PEAKS * channels).max(start);
        let mut peak = 0.0f32;
        for sample in &pcm.samples[start..end] {
            peak = peak.max(sample.abs());
        }
        *level = (peak.min(1.0) * 255.0).round() as u8;
    }
    Peaks(levels)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeakCache {
    directory: PathBuf,
}

impl PeakCache {
    pub fn new(directory: PathBuf) -> PeakCache {
        PeakCache { directory }
    }

    fn path(&self, recording: i64) -> PathBuf {
        self.directory.join(format!("{recording}.peaks"))
    }

    pub fn read(&self, recording: i64) -> Option<Peaks> {
        let bytes = std::fs::read(self.path(recording)).ok()?;
        let levels: [u8; PEAKS] = bytes.try_into().ok()?;
        Some(Peaks(levels))
    }

    pub fn write(&self, recording: i64, peaks: &Peaks) -> Result<(), String> {
        let Peaks(levels) = peaks;
        std::fs::create_dir_all(&self.directory).map_err(|error| error.to_string())?;
        let target = self.path(recording);
        let staging = staging_path(&target);
        std::fs::write(&staging, levels).map_err(|error| error.to_string())?;
        std::fs::rename(&staging, &target).map_err(|error| error.to_string())
    }
}

fn staging_path(target: &Path) -> PathBuf {
    let mut staging = target.as_os_str().to_owned();
    staging.push(".tmp");
    PathBuf::from(staging)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub rate: u32,
}

impl Pcm {
    pub fn duration(&self) -> Duration {
        let channels = u64::from(self.channels.max(1));
        let frames = u64::try_from(self.samples.len()).unwrap_or(u64::MAX) / channels;
        Duration::from_secs_f64(frames as f64 / f64::from(self.rate.max(1)))
    }
}

pub fn decode(mime: &str, bytes: Vec<u8>) -> Result<Pcm, String> {
    match mime {
        "audio/ogg" | "audio/opus" => decode_opus(bytes),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" | "audio/aac" => decode_with_rodio(bytes, "m4a"),
        "audio/mpeg" | "audio/mp3" => decode_with_rodio(bytes, "mp3"),
        "audio/wav" | "audio/x-wav" => decode_with_rodio(bytes, "wav"),
        other => match sniff(&bytes) {
            Some(Container::Ogg) => decode_opus(bytes),
            Some(Container::Mp4) => decode_with_rodio(bytes, "m4a"),
            None => Err(format!("cannot play {other}")),
        },
    }
}

enum Container {
    Ogg,
    Mp4,
}

fn sniff(bytes: &[u8]) -> Option<Container> {
    if bytes.starts_with(b"OggS") {
        return Some(Container::Ogg);
    }
    if bytes.get(4..8) == Some(b"ftyp".as_slice()) {
        return Some(Container::Mp4);
    }
    None
}

fn decode_opus(bytes: Vec<u8>) -> Result<Pcm, String> {
    let mut reader = OggOpusReader::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let channels = usize::from(reader.head().channel_count);
    let mut decoder = reader
        .head()
        .decoder(OPUS_RATE)
        .map_err(|error| error.to_string())?;
    let mut trim =
        Trim::new(reader.head(), OPUS_RATE, channels).map_err(|error| error.to_string())?;
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    let mut samples = Vec::new();
    for packet in reader.packets() {
        let packet = packet.map_err(|error| error.to_string())?;
        let decoded = decoder
            .decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)
            .map_err(|error| error.to_string())?;
        samples.extend_from_slice(trim.keep(&packet, &block[..decoded * channels]));
    }
    Ok(Pcm {
        samples,
        channels: u16::try_from(channels).unwrap_or(1),
        rate: u32::try_from(OPUS_RATE).unwrap_or(48_000),
    })
}

fn decode_with_rodio(bytes: Vec<u8>, hint: &str) -> Result<Pcm, String> {
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let decoder = Decoder::builder()
        .with_data(Cursor::new(bytes))
        .with_byte_len(length)
        .with_seekable(true)
        .with_hint(hint)
        .build()
        .map_err(|error| error.to_string())?;
    let channels = decoder.channels().get();
    let rate = decoder.sample_rate().get();
    let mut samples = Vec::new();
    for sample in decoder {
        samples.push(sample);
    }
    Ok(Pcm {
        samples,
        channels,
        rate,
    })
}

pub trait Speaker {
    fn start(&mut self, pcm: Pcm) -> Result<(), String>;
    fn stop(&mut self);
    fn position(&self) -> Duration;
    fn finished(&self) -> bool;
}

#[derive(Default)]
pub struct RodioSpeaker {
    sink: Option<MixerDeviceSink>,
    player: Option<Player>,
}

impl Speaker for RodioSpeaker {
    fn start(&mut self, pcm: Pcm) -> Result<(), String> {
        self.stop();
        if self.sink.is_none() {
            let mut sink =
                DeviceSinkBuilder::open_default_sink().map_err(|error| error.to_string())?;
            sink.log_on_drop(false);
            self.sink = Some(sink);
        }
        let Some(sink) = &self.sink else {
            return Err("no audio output".to_string());
        };
        let Some(channels) = NonZero::new(pcm.channels) else {
            return Err("the recording has no channels".to_string());
        };
        let Some(rate) = NonZero::new(pcm.rate) else {
            return Err("the recording has no sample rate".to_string());
        };
        let player = Player::connect_new(sink.mixer());
        player.append(SamplesBuffer::new(channels, rate, pcm.samples));
        self.player = Some(player);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(player) = self.player.take() {
            player.stop();
        }
    }

    fn position(&self) -> Duration {
        match &self.player {
            Some(player) => player.get_pos(),
            None => Duration::ZERO,
        }
    }

    fn finished(&self) -> bool {
        match &self.player {
            Some(player) => player.empty(),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TONE_OGG: &[u8] = include_bytes!("../../core/testdata/v3/media/tone.ogg");
    const TONE_M4A: &[u8] = include_bytes!("../../core/testdata/v3/media/tone.m4a");

    fn loudness(pcm: &Pcm) -> f32 {
        let mut peak = 0.0f32;
        for sample in &pcm.samples {
            peak = peak.max(sample.abs());
        }
        peak
    }

    #[test]
    fn an_opus_recording_decodes_to_its_length() {
        let pcm = decode("audio/ogg", TONE_OGG.to_vec()).expect("the tone decodes");
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.rate, 48_000);
        let seconds = pcm.duration().as_secs_f64();
        assert!((seconds - 3.0).abs() < 0.05, "{seconds}");
        assert!(loudness(&pcm) > 0.1, "the tone is audible");
    }

    #[test]
    fn an_m4a_recording_decodes_to_its_length() {
        let pcm = decode("audio/mp4", TONE_M4A.to_vec()).expect("the tone decodes");
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.rate, 44_100);
        let seconds = pcm.duration().as_secs_f64();
        assert!((seconds - 2.0).abs() < 0.1, "{seconds}");
        assert!(loudness(&pcm) > 0.1, "the tone is audible");
    }

    #[test]
    fn unknown_or_broken_media_is_an_error() {
        assert!(decode("video/webm", b"not a recording".to_vec()).is_err());
        assert!(decode("audio/ogg", b"not an ogg".to_vec()).is_err());
        assert!(decode("audio/mp4", b"not an mp4".to_vec()).is_err());
    }

    #[test]
    fn an_untyped_recording_is_recognised_by_its_container() {
        let ogg = decode("application/octet-stream", TONE_OGG.to_vec()).expect("ogg sniffed");
        assert_eq!(ogg.rate, 48_000);
        let m4a = decode("application/octet-stream", TONE_M4A.to_vec()).expect("m4a sniffed");
        assert_eq!(m4a.rate, 44_100);
    }

    #[test]
    fn peaks_are_sixty_four_absolute_levels() {
        let pcm = decode("audio/ogg", TONE_OGG.to_vec()).expect("the tone decodes");
        let Peaks(levels) = peaks(&pcm);
        let tone = (loudness(&pcm).min(1.0) * 255.0).round() as u8;
        let mut loudest = 0;
        for level in levels {
            loudest = loudest.max(level);
            assert!(
                level > tone / 2,
                "a steady tone fills every bucket: {levels:?}"
            );
        }
        assert_eq!(loudest, tone);
        let silence = Pcm {
            samples: vec![0.0; 4_800],
            channels: 1,
            rate: 48_000,
        };
        assert_eq!(peaks(&silence), Peaks([0; PEAKS]));
        let empty = Pcm {
            samples: Vec::new(),
            channels: 2,
            rate: 48_000,
        };
        assert_eq!(peaks(&empty), Peaks([0; PEAKS]));
    }

    #[test]
    fn the_peak_cache_round_trips_and_rejects_a_wrong_size() {
        let directory = std::env::temp_dir().join(format!("tuclaw-peaks-{}", std::process::id()));
        let cache = PeakCache::new(directory.clone());
        assert_eq!(cache.read(3), None);
        let mut levels = [0u8; PEAKS];
        for (index, level) in levels.iter_mut().enumerate() {
            *level = index as u8 * 4;
        }
        cache.write(3, &Peaks(levels)).expect("written");
        assert_eq!(cache.read(3), Some(Peaks(levels)));
        std::fs::write(directory.join("4.peaks"), [1u8; 10]).expect("written");
        assert_eq!(cache.read(4), None);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_speaker_without_a_player_reports_finished() {
        let speaker = RodioSpeaker::default();
        assert!(speaker.finished());
        assert_eq!(speaker.position(), Duration::ZERO);
    }
}
