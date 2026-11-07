use std::{
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use windows::Win32::{
    Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioCaptureClient,
        IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eMultimedia, eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    },
};

const SAMPLE_BYTES: usize = 2;
const IDLE_PACKET_MS: u64 = 100;
const DEVICE_ERROR: &str = "无法采集 Windows 默认扬声器的音频。请检查音频输出设备。";

struct ComApartment;

impl ComApartment {
    fn new() -> windows::core::Result<Self> {
        // This runs on a capture worker. COM is balanced on that same thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

impl Loopback {
    fn start(sample_rate: u32) -> windows::core::Result<Self> {
        let format = WAVEFORMATEX {
            wFormatTag: 1, // WAVE_FORMAT_PCM
            nChannels: 1,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate * SAMPLE_BYTES as u32,
            nBlockAlign: SAMPLE_BYTES as u16,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // The apartment outlives both interfaces. WASAPI handles the device's mix conversion.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                1_000_000, // 100 ms in 100 ns units
                0,
                &format,
                None,
            )?;
            let capture = client.GetService()?;
            client.Start()?;
            Ok(Self { client, capture })
        }
    }

    fn next_packet(&self) -> windows::core::Result<Option<Vec<u8>>> {
        // Read and release the WASAPI packet on the same capture thread.
        unsafe {
            if self.capture.GetNextPacketSize()? == 0 {
                return Ok(None);
            }
            let mut data = ptr::null_mut();
            let mut frames = 0;
            let mut flags = 0;
            self.capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            if frames == 0 {
                return Ok(None);
            }
            let len = frames as usize * SAMPLE_BYTES;
            let packet = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0; len]
            } else {
                std::slice::from_raw_parts(data, len).to_vec()
            };
            self.capture.ReleaseBuffer(frames)?;
            Ok(Some(packet))
        }
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

pub fn capture(
    sample_rate: u32,
    stop: Arc<AtomicBool>,
    ready: oneshot::Sender<()>,
    audio: mpsc::Sender<Vec<u8>>,
    mut on_packet: impl FnMut(&[u8]) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let _apartment = ComApartment::new().map_err(|_| DEVICE_ERROR)?;
    let loopback = Loopback::start(sample_rate).map_err(|_| DEVICE_ERROR)?;
    if ready.send(()).is_err() {
        return Ok(());
    }
    let mut last_packet = Instant::now();
    let idle_period = Duration::from_millis(IDLE_PACKET_MS);
    while !stop.load(Ordering::Acquire) {
        let packet = match loopback.next_packet().map_err(|_| DEVICE_ERROR)? {
            Some(packet) => packet,
            // An idle output device may stop delivering packets. Preserve long gaps
            // as silence so the recognizer sees a pause even with no active player.
            None if last_packet.elapsed() >= idle_period => {
                vec![0; sample_rate as usize * SAMPLE_BYTES * IDLE_PACKET_MS as usize / 1000]
            }
            None => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
        };
        last_packet = Instant::now();
        if stop.load(Ordering::Acquire) {
            break;
        }
        on_packet(&packet)?;
        match audio.try_send(packet) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => break,
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err("网络发送跟不上音频采集，已停止转写。");
            }
        }
    }
    Ok(())
}

pub fn level(packet: &[u8]) -> f32 {
    packet
        .as_chunks::<2>()
        .0
        .iter()
        .map(|sample| i16::from_le_bytes(*sample).unsigned_abs())
        .max()
        .unwrap_or_default() as f32
        / 32768.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_handles_pcm_extremes_and_silence() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0; 4]), 0.0);
        assert_eq!(level(&i16::MIN.to_le_bytes()), 1.0);
        assert_eq!(level(&i16::MAX.to_le_bytes()), 32767.0 / 32768.0);
    }
}
