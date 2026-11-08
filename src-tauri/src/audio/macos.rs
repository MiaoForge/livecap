use crate::{
    audio_feed::AudioFeed,
    audio_mixer::{Mixer, Source},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::oneshot;

const CAPTURE_ERROR: &str = "无法采集 macOS 系统音频。请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制中允许 Livecap，然后重新启动应用。";

type PacketCallback = unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const u8, usize) -> i32;
type StateCallback = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;

unsafe extern "C" {
    fn livecap_request_audio_access(include_microphone: i32) -> i32;
    fn livecap_capture_audio(
        rate: u32,
        include_microphone: i32,
        context: *mut std::ffi::c_void,
        packet: PacketCallback,
        stopped: StateCallback,
        ready: StateCallback,
    ) -> i32;
}

pub fn request_access(include_microphone: bool) -> Result<(), &'static str> {
    match unsafe { livecap_request_audio_access(i32::from(include_microphone)) } {
        0 => Ok(()),
        2 => Err(
            "macOS 尚未允许麦克风采集。请在系统设置 → 隐私与安全性 → 麦克风中允许 Livecap 或启动开发版的终端，然后完全退出并重新打开应用；也可关闭会议的麦克风选项，仅录制系统声音。",
        ),
        _ => Err(
            "macOS 尚未允许系统音频采集。请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制中允许 Livecap 或启动开发版的终端，然后完全退出并重新打开应用。",
        ),
    }
}

struct Capture<'a> {
    stop: Arc<AtomicBool>,
    ready: Option<oneshot::Sender<()>>,
    audio: Arc<AudioFeed>,
    on_packet: &'a mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
    error: Option<&'static str>,
    finished: bool,
    mixer: Option<Mixer>,
}

impl Capture<'_> {
    fn input(&mut self, source: i32, pcm: &[u8]) -> Result<(), &'static str> {
        if let Some(mixer) = self.mixer.as_mut() {
            let (paused, generation) = self.audio.capture_state()?;
            mixer.synchronize(generation);
            if paused {
                return Ok(());
            }
            match source {
                0 => mixer.push(Source::System, pcm),
                1 => mixer.push(Source::Microphone, pcm),
                2 => {
                    let generation = mixer.generation();
                    let mixed = mixer.render();
                    self.finished =
                        !self
                            .audio
                            .forward_mixed(&mixed, generation, &mut self.on_packet)?;
                    Ok(())
                }
                _ => Err("无法识别音频采集来源。"),
            }
        } else if source == 0 {
            self.packet(pcm)
        } else {
            Err("无法识别音频采集来源。")
        }
    }

    fn packet(&mut self, packet: &[u8]) -> Result<(), &'static str> {
        self.finished = !self.audio.forward(packet, &mut self.on_packet)?;
        Ok(())
    }
}

unsafe extern "C" fn packet(
    context: *mut std::ffi::c_void,
    source: i32,
    bytes: *const u8,
    len: usize,
) -> i32 {
    // Swift serializes callbacks and drains its queue before the FFI call returns.
    let state = unsafe { &mut *context.cast::<Capture<'_>>() };
    if state.stop.load(Ordering::Acquire) || state.finished {
        return 1;
    }
    let bytes = if source == 2 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| state.input(source, bytes))) {
        Ok(Ok(())) => 0,
        result => {
            state.error = Some(match result {
                Ok(Err(error)) => error,
                _ => "系统音频处理意外停止。",
            });
            state.finished = true;
            1
        }
    }
}

unsafe extern "C" fn stopped(context: *mut std::ffi::c_void) -> i32 {
    let state = unsafe { &*context.cast::<Capture<'_>>() };
    i32::from(state.stop.load(Ordering::Acquire) || state.finished)
}

unsafe extern "C" fn ready(context: *mut std::ffi::c_void) -> i32 {
    let state = unsafe { &mut *context.cast::<Capture<'_>>() };
    state.finished = state.stop.load(Ordering::Acquire)
        || state
            .ready
            .take()
            .is_none_or(|ready| ready.send(()).is_err());
    i32::from(state.finished)
}

pub fn capture(
    sample_rate: u32,
    include_microphone: bool,
    stop: Arc<AtomicBool>,
    ready_tx: oneshot::Sender<()>,
    audio: Arc<AudioFeed>,
    mut on_packet: impl FnMut(&[u8]) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut state = Capture {
        stop,
        ready: Some(ready_tx),
        audio,
        on_packet: &mut on_packet,
        error: None,
        finished: false,
        mixer: include_microphone.then(|| Mixer::new(sample_rate)),
    };
    let result = unsafe {
        livecap_capture_audio(
            sample_rate,
            i32::from(include_microphone),
            (&mut state as *mut Capture<'_>).cast(),
            packet,
            stopped,
            ready,
        )
    };
    if let Some(error) = state.error {
        Err(error)
    } else if result != 0 && !state.finished && !state.stop.load(Ordering::Acquire) {
        Err(if result == 3 {
            "无法采集麦克风音频。请检查麦克风权限、系统默认输入设备及是否被其他应用占用；也可关闭会议的麦克风选项。"
        } else {
            CAPTURE_ERROR
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_feed::Packet;
    use tokio::sync::mpsc;

    #[test]
    fn microphone_and_system_callbacks_mix_on_one_clock_and_reset_at_pause() {
        let (sender, mut packets) = mpsc::channel(8);
        let feed = Arc::new(AudioFeed::new(sender));
        let mut recorded = Vec::new();
        let mut record = |bytes: &[u8]| {
            recorded.extend_from_slice(bytes);
            Ok(())
        };
        let mut state = Capture {
            stop: Arc::new(AtomicBool::new(false)),
            ready: None,
            audio: feed.clone(),
            on_packet: &mut record,
            error: None,
            finished: false,
            mixer: Some(Mixer::new(100)),
        };
        let context = (&mut state as *mut Capture<'_>).cast();
        let system: Vec<u8> = [100_i16, 200]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let microphone: Vec<u8> = [300_i16, 400]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(
            unsafe { packet(context, 0, system.as_ptr(), system.len()) },
            0
        );
        assert_eq!(
            unsafe { packet(context, 1, microphone.as_ptr(), microphone.len()) },
            0
        );
        assert!(
            packets.try_recv().is_err(),
            "raw inputs must not be concatenated into the output"
        );
        assert_eq!(unsafe { packet(context, 2, std::ptr::null(), 0) }, 0);
        let expected: Vec<u8> = [400_i16, 600]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(packets.try_recv().unwrap(), Packet::Pcm(expected.clone()));
        feed.set_paused(true).unwrap();
        assert_eq!(
            unsafe { packet(context, 1, microphone.as_ptr(), microphone.len()) },
            0
        );
        feed.set_paused(false).unwrap();
        assert_eq!(unsafe { packet(context, 2, std::ptr::null(), 0) }, 0);
        assert_eq!(packets.try_recv().unwrap(), Packet::Paused);
        assert_eq!(packets.try_recv().unwrap(), Packet::Resumed);
        assert_eq!(packets.try_recv().unwrap(), Packet::Pcm(vec![0; 4]));
        drop(state);
        assert_eq!(recorded, [expected, vec![0; 4]].concat());
    }

    #[test]
    fn forwards_recording_and_reports_backpressure() {
        let (audio, mut packets) = mpsc::channel(1);
        let (ready, _) = oneshot::channel();
        let mut recorded = Vec::new();
        let mut record = |bytes: &[u8]| {
            recorded.extend_from_slice(bytes);
            Ok(())
        };
        let mut state = Capture {
            stop: Arc::new(AtomicBool::new(false)),
            ready: Some(ready),
            audio: Arc::new(AudioFeed::new(audio)),
            on_packet: &mut record,
            error: None,
            finished: false,
            mixer: None,
        };
        assert!(state.packet(&[1, 2]).is_ok());
        assert!(state.packet(&[3, 4]).is_err());
        assert_eq!(packets.try_recv().unwrap(), Packet::Pcm(vec![1, 2]));
        drop(state);
        assert_eq!(recorded, [1, 2, 3, 4]);
    }

    #[test]
    fn closed_consumer_stops_capture() {
        let (audio, packets) = mpsc::channel(1);
        drop(packets);
        let mut record = |_: &[u8]| Ok(());
        let mut state = Capture {
            stop: Arc::new(AtomicBool::new(false)),
            ready: None,
            audio: Arc::new(AudioFeed::new(audio)),
            on_packet: &mut record,
            error: None,
            finished: false,
            mixer: None,
        };
        assert!(state.packet(&[0, 0]).is_ok());
        assert!(state.finished);
    }

    #[test]
    fn cancelled_readiness_and_stop_prevent_delivery() {
        let (audio, mut packets) = mpsc::channel(1);
        let (ready_tx, ready_rx) = oneshot::channel();
        drop(ready_rx);
        let stop = Arc::new(AtomicBool::new(false));
        let mut record =
            |_: &[u8]| -> Result<(), &'static str> { panic!("must not deliver audio") };
        let mut state = Capture {
            stop: stop.clone(),
            ready: Some(ready_tx),
            audio: Arc::new(AudioFeed::new(audio)),
            on_packet: &mut record,
            error: None,
            finished: false,
            mixer: None,
        };
        let context = (&mut state as *mut Capture<'_>).cast();
        assert_eq!(unsafe { ready(context) }, 1);
        assert_eq!(unsafe { stopped(context) }, 1);
        let bytes = [0, 0];
        assert_eq!(
            unsafe { packet(context, 0, bytes.as_ptr(), bytes.len()) },
            1
        );
        assert!(packets.try_recv().is_err());
        state.finished = false;
        stop.store(true, Ordering::Release);
        assert_eq!(unsafe { stopped(context) }, 1);
    }

    #[test]
    fn recorder_failure_is_preserved_across_native_callback() {
        let (audio, _packets) = mpsc::channel(1);
        let mut record = |_: &[u8]| Err("recording failed");
        let mut state = Capture {
            stop: Arc::new(AtomicBool::new(false)),
            ready: None,
            audio: Arc::new(AudioFeed::new(audio)),
            on_packet: &mut record,
            error: None,
            finished: false,
            mixer: None,
        };
        let context = (&mut state as *mut Capture<'_>).cast();
        let bytes = [0, 0];
        assert_eq!(
            unsafe { packet(context, 0, bytes.as_ptr(), bytes.len()) },
            1
        );
        assert_eq!(state.error, Some("recording failed"));
        assert!(state.finished);
    }
}
