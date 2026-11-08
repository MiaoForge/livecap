use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, oneshot};

const CAPTURE_ERROR: &str = "无法采集 macOS 系统音频。请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制中允许 Livecap，然后重新启动应用。";

type PacketCallback = unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> i32;
type StateCallback = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;

unsafe extern "C" {
    fn livecap_request_audio_access() -> i32;
    fn livecap_capture_audio(
        rate: u32,
        context: *mut std::ffi::c_void,
        packet: PacketCallback,
        stopped: StateCallback,
        ready: StateCallback,
    ) -> i32;
}

pub fn request_access() -> Result<(), &'static str> {
    if unsafe { livecap_request_audio_access() } == 0 {
        Ok(())
    } else {
        Err(
            "macOS 尚未允许系统音频采集。请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制中允许 Livecap 或启动开发版的终端，然后完全退出并重新运行 npm run tauri dev（安装版请重新打开 Livecap）。",
        )
    }
}

struct Capture<'a> {
    stop: Arc<AtomicBool>,
    ready: Option<oneshot::Sender<()>>,
    audio: mpsc::Sender<Vec<u8>>,
    on_packet: &'a mut dyn FnMut(&[u8]) -> Result<(), &'static str>,
    error: Option<&'static str>,
    finished: bool,
}

impl Capture<'_> {
    fn packet(&mut self, packet: &[u8]) -> Result<(), &'static str> {
        (self.on_packet)(packet)?;
        match self.audio.try_send(packet.to_vec()) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.finished = true;
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err("网络发送跟不上音频采集，已停止转写。"),
        }
    }
}

unsafe extern "C" fn packet(context: *mut std::ffi::c_void, bytes: *const u8, len: usize) -> i32 {
    // Swift serializes callbacks and drains its queue before the FFI call returns.
    let state = unsafe { &mut *context.cast::<Capture<'_>>() };
    if state.stop.load(Ordering::Acquire) || state.finished {
        return 1;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| state.packet(bytes))) {
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
    stop: Arc<AtomicBool>,
    ready_tx: oneshot::Sender<()>,
    audio: mpsc::Sender<Vec<u8>>,
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
    };
    let result = unsafe {
        livecap_capture_audio(
            sample_rate,
            (&mut state as *mut Capture<'_>).cast(),
            packet,
            stopped,
            ready,
        )
    };
    if let Some(error) = state.error {
        Err(error)
    } else if result != 0 && !state.finished && !state.stop.load(Ordering::Acquire) {
        Err(CAPTURE_ERROR)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            audio,
            on_packet: &mut record,
            error: None,
            finished: false,
        };
        assert!(state.packet(&[1, 2]).is_ok());
        assert!(state.packet(&[3, 4]).is_err());
        assert_eq!(packets.try_recv().unwrap(), [1, 2]);
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
            audio,
            on_packet: &mut record,
            error: None,
            finished: false,
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
            audio,
            on_packet: &mut record,
            error: None,
            finished: false,
        };
        let context = (&mut state as *mut Capture<'_>).cast();
        assert_eq!(unsafe { ready(context) }, 1);
        assert_eq!(unsafe { stopped(context) }, 1);
        let bytes = [0, 0];
        assert_eq!(unsafe { packet(context, bytes.as_ptr(), bytes.len()) }, 1);
        assert!(packets.try_recv().is_err());
        state.finished = false;
        stop.store(true, Ordering::Release);
        assert_eq!(unsafe { stopped(context) }, 1);
    }

    #[test]
    fn recorder_failure_is_preserved_across_native_callback() {
        let (audio, _) = mpsc::channel(1);
        let mut record = |_: &[u8]| Err("recording failed");
        let mut state = Capture {
            stop: Arc::new(AtomicBool::new(false)),
            ready: None,
            audio,
            on_packet: &mut record,
            error: None,
            finished: false,
        };
        let context = (&mut state as *mut Capture<'_>).cast();
        let bytes = [0, 0];
        assert_eq!(unsafe { packet(context, bytes.as_ptr(), bytes.len()) }, 1);
        assert_eq!(state.error, Some("recording failed"));
        assert!(state.finished);
    }
}
