use std::sync::Mutex;
use tokio::sync::mpsc;

#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    Pcm(Vec<u8>),
    Paused,
    Resumed,
}

struct State {
    paused: bool,
    sender: mpsc::Sender<Packet>,
}

/// Serializes pause boundaries with recording and provider delivery. Paused audio
/// is discarded before either consumer, keeping their sample timelines identical.
pub struct AudioFeed(Mutex<State>);

impl AudioFeed {
    pub fn new(sender: mpsc::Sender<Packet>) -> Self {
        Self(Mutex::new(State {
            paused: false,
            sender,
        }))
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), &'static str> {
        let mut state = self.0.lock().map_err(|_| "无法读取录音状态。")?;
        if state.paused == paused {
            return Ok(());
        }
        let packet = if paused {
            Packet::Paused
        } else {
            Packet::Resumed
        };
        state
            .sender
            .try_send(packet)
            .map_err(|_| "无法切换录音状态，请稍后重试或结束会话。")?;
        state.paused = paused;
        Ok(())
    }

    pub fn forward(
        &self,
        packet: &[u8],
        record: impl FnOnce(&[u8]) -> Result<(), &'static str>,
    ) -> Result<bool, &'static str> {
        let state = self.0.lock().map_err(|_| "无法读取录音状态。")?;
        if state.sender.is_closed() {
            return Ok(false);
        }
        if state.paused {
            return Ok(true);
        }
        record(packet)?;
        match state.sender.try_send(Packet::Pcm(packet.to_vec())) {
            Ok(()) => Ok(true),
            Err(mpsc::error::TrySendError::Closed(_)) => Ok(false),
            Err(mpsc::error::TrySendError::Full(_)) => Err("网络发送跟不上音频采集，已停止转写。"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_discards_audio_for_both_recording_and_provider_and_preserves_boundaries() {
        let (tx, mut rx) = mpsc::channel(8);
        let feed = AudioFeed::new(tx);
        let mut recorded = Vec::new();
        feed.forward(&[1, 2], |bytes| {
            recorded.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        feed.set_paused(true).unwrap();
        feed.set_paused(true).unwrap();
        assert!(
            feed.forward(&[3, 4], |_| panic!("paused audio must not be recorded"))
                .unwrap()
        );
        feed.set_paused(false).unwrap();
        feed.forward(&[5, 6], |bytes| {
            recorded.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(recorded, [1, 2, 5, 6]);
        assert_eq!(rx.try_recv().unwrap(), Packet::Pcm(vec![1, 2]));
        assert_eq!(rx.try_recv().unwrap(), Packet::Paused);
        assert_eq!(rx.try_recv().unwrap(), Packet::Resumed);
        assert_eq!(rx.try_recv().unwrap(), Packet::Pcm(vec![5, 6]));
        assert!(rx.try_recv().is_err());
        feed.set_paused(true).unwrap();
        drop(feed);
        assert_eq!(rx.try_recv().unwrap(), Packet::Paused);
        assert!(rx.blocking_recv().is_none());
    }

    #[test]
    fn failed_pause_does_not_change_recording_state() {
        let (tx, mut rx) = mpsc::channel(1);
        let feed = AudioFeed::new(tx);
        feed.forward(&[1, 2], |_| Ok(())).unwrap();
        assert!(feed.set_paused(true).is_err());
        rx.try_recv().unwrap();
        feed.forward(&[3, 4], |_| Ok(())).unwrap();
        assert_eq!(rx.try_recv().unwrap(), Packet::Pcm(vec![3, 4]));
        drop(rx);
        assert!(
            !feed
                .forward(&[5, 6], |_| panic!("closed stream must not record"))
                .unwrap()
        );
    }
}
