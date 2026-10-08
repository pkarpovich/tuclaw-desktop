use std::time::Duration;

use tuclaw_core::v3::AudioKind;

pub const LIMIT: Duration = Duration::from_secs(10 * 60);

pub struct Take {
    pub kind: AudioKind,
    pub bytes: Vec<u8>,
}

pub trait Recorder {
    fn start(&mut self) -> Result<(), String>;
    fn finish(&mut self) -> Result<Take, String>;
    fn cancel(&mut self);
    fn level(&self) -> Option<f32> {
        None
    }
}

#[derive(Default)]
pub struct NoRecorder;

impl Recorder for NoRecorder {
    fn start(&mut self) -> Result<(), String> {
        Err("recording is not available here".to_string())
    }

    fn finish(&mut self) -> Result<Take, String> {
        Err("nothing is being recorded".to_string())
    }

    fn cancel(&mut self) {}
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use apple::AvRecorder;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple {
    use std::path::PathBuf;

    use objc2::AllocAnyThread;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_avf_audio::{
        AVAudioRecorder, AVEncoderAudioQualityKey, AVFormatIDKey, AVNumberOfChannelsKey,
        AVSampleRateKey,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
    use tuclaw_core::v3::AudioKind;

    use super::{LIMIT, Recorder, Take};

    const MPEG4_AAC: u32 = u32::from_be_bytes(*b"aac ");
    const QUIET_DB: f32 = -50.;
    const SAMPLE_RATE: f64 = 44_100.;
    const HIGH_QUALITY: i64 = 0x60;

    #[derive(Default)]
    pub struct AvRecorder {
        live: Option<Retained<AVAudioRecorder>>,
    }

    impl AvRecorder {
        fn path() -> PathBuf {
            std::env::temp_dir().join(format!("tuclaw-recording-{}.m4a", std::process::id()))
        }
    }

    impl Recorder for AvRecorder {
        fn start(&mut self) -> Result<(), String> {
            self.cancel();
            #[cfg(target_os = "ios")]
            crate::audio_session::begin_recording()?;
            let path = AvRecorder::path();
            let Some(url) = NSURL::from_file_path(&path) else {
                return Err("the recording has nowhere to go".to_string());
            };
            let (Some(format_key), Some(rate_key), Some(channels_key), Some(quality_key)) = (
                unsafe { AVFormatIDKey },
                unsafe { AVSampleRateKey },
                unsafe { AVNumberOfChannelsKey },
                unsafe { AVEncoderAudioQualityKey },
            ) else {
                return Err("AVFoundation is missing its settings keys".to_string());
            };
            let format = NSNumber::new_u32(MPEG4_AAC);
            let rate = NSNumber::new_f64(SAMPLE_RATE);
            let channels = NSNumber::new_u32(1);
            let quality = NSNumber::new_i64(HIGH_QUALITY);
            let values: [&AnyObject; 4] = [&format, &rate, &channels, &quality];
            let settings: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
                &[format_key, rate_key, channels_key, quality_key],
                &values,
            );
            let recorder = unsafe {
                AVAudioRecorder::initWithURL_settings_error(
                    AVAudioRecorder::alloc(),
                    &url,
                    &settings,
                )
            }
            .map_err(|error| error.localizedDescription().to_string())?;
            unsafe { recorder.setMeteringEnabled(true) };
            let started = unsafe { recorder.recordForDuration(LIMIT.as_secs_f64()) };
            if !started {
                #[cfg(target_os = "ios")]
                crate::audio_session::end_recording();
                return Err("the microphone is not available".to_string());
            }
            self.live = Some(recorder);
            Ok(())
        }

        fn finish(&mut self) -> Result<Take, String> {
            let Some(recorder) = self.live.take() else {
                return Err("nothing is being recorded".to_string());
            };
            unsafe { recorder.stop() };
            #[cfg(target_os = "ios")]
            crate::audio_session::end_recording();
            let path = AvRecorder::path();
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            std::fs::remove_file(&path).ok();
            Ok(Take {
                kind: AudioKind::M4a,
                bytes,
            })
        }

        fn level(&self) -> Option<f32> {
            let recorder = self.live.as_ref()?;
            let decibels = unsafe {
                recorder.updateMeters();
                recorder.averagePowerForChannel(0)
            };
            Some(((decibels - QUIET_DB) / -QUIET_DB).clamp(0., 1.))
        }

        fn cancel(&mut self) {
            let Some(recorder) = self.live.take() else {
                return;
            };
            unsafe {
                recorder.stop();
                recorder.deleteRecording();
            }
            #[cfg(target_os = "ios")]
            crate::audio_session::end_recording();
        }
    }
}
