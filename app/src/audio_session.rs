use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_avf_audio::{
    AVAudioApplication, AVAudioApplicationRecordPermission, AVAudioSession,
    AVAudioSessionCategoryOptions, AVAudioSessionCategoryPlayAndRecord,
    AVAudioSessionCategoryPlayback,
};

pub fn use_for_playback() {
    let Some(playback) = (unsafe { AVAudioSessionCategoryPlayback }) else {
        return;
    };
    let session = unsafe { AVAudioSession::sharedInstance() };
    unsafe {
        session
            .setCategory_withOptions_error(playback, AVAudioSessionCategoryOptions::empty())
            .ok();
    }
}

pub fn begin_recording() -> Result<(), String> {
    let permission = unsafe { AVAudioApplication::sharedInstance().recordPermission() };
    if permission == AVAudioApplicationRecordPermission::Undetermined {
        let answered = RcBlock::new(|_granted: Bool| {});
        unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&answered) };
        return Err("allow the microphone, then tap again".to_string());
    }
    if permission != AVAudioApplicationRecordPermission::Granted {
        return Err("microphone access is off in Settings".to_string());
    }
    let Some(record) = (unsafe { AVAudioSessionCategoryPlayAndRecord }) else {
        return Err("AVFoundation has no record category".to_string());
    };
    let session = unsafe { AVAudioSession::sharedInstance() };
    unsafe {
        session
            .setCategory_withOptions_error(record, AVAudioSessionCategoryOptions::DefaultToSpeaker)
            .map_err(|error| error.localizedDescription().to_string())?;
        session
            .setActive_error(true)
            .map_err(|error| error.localizedDescription().to_string())?;
    }
    Ok(())
}

pub fn end_recording() {
    let session = unsafe { AVAudioSession::sharedInstance() };
    unsafe { session.setActive_error(false).ok() };
    use_for_playback();
}
