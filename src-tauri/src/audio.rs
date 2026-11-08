#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::capture;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{capture, request_access};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
compile_error!("Livecap supports Windows and macOS.");

#[cfg(target_os = "windows")]
pub fn request_access() -> Result<(), &'static str> {
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
