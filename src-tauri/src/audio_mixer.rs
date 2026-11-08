use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    System,
    Microphone,
}

/// Mixes two independently packetized mono PCM16 sources on one 20 ms clock.
/// An absent source contributes silence. A bounded queue prevents device clock
/// drift or a delayed callback from accumulating unbounded playback latency.
pub struct Mixer {
    system: VecDeque<i16>,
    microphone: VecDeque<i16>,
    frame_samples: usize,
    max_samples: usize,
    generation: u64,
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            system: VecDeque::new(),
            microphone: VecDeque::new(),
            frame_samples: sample_rate as usize / 50,
            max_samples: sample_rate as usize / 4,
            generation: 0,
        }
    }

    pub fn synchronize(&mut self, generation: u64) {
        if generation != self.generation {
            self.system.clear();
            self.microphone.clear();
            self.generation = generation;
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn push(&mut self, source: Source, pcm: &[u8]) -> Result<(), &'static str> {
        let (samples, remainder) = pcm.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err("音频采集返回了不完整的 PCM 数据。");
        }
        let queue = match source {
            Source::System => &mut self.system,
            Source::Microphone => &mut self.microphone,
        };
        for sample in samples {
            if queue.len() == self.max_samples {
                queue.pop_front();
            }
            queue.push_back(i16::from_le_bytes(*sample));
        }
        Ok(())
    }

    pub fn render(&mut self) -> Vec<u8> {
        let mut pcm = Vec::with_capacity(self.frame_samples * 2);
        for _ in 0..self.frame_samples {
            let system = i32::from(self.system.pop_front().unwrap_or_default());
            let microphone = i32::from(self.microphone.pop_front().unwrap_or_default());
            let sample = (system + microphone).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        pcm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn mixes_simultaneous_audio_without_doubling_duration_and_clamps_peaks() {
        let mut mixer = Mixer::new(200);
        mixer
            .push(Source::System, &pcm(&[1000, i16::MAX, i16::MIN, 2000]))
            .unwrap();
        mixer
            .push(Source::Microphone, &pcm(&[500, 1000, -1000, -1000]))
            .unwrap();
        assert_eq!(mixer.render(), pcm(&[1500, i16::MAX, i16::MIN, 1000]));
        assert_eq!(mixer.render(), vec![0; 8]);
    }

    #[test]
    fn lone_microphone_or_system_audio_keeps_its_original_gain() {
        for source in [Source::System, Source::Microphone] {
            let mut mixer = Mixer::new(100);
            mixer.push(source, &pcm(&[1234, -2345])).unwrap();
            assert_eq!(mixer.render(), pcm(&[1234, -2345]));
        }
    }

    #[test]
    fn packet_boundaries_do_not_change_the_mixed_sample_clock() {
        for rate in [16_000, 24_000] {
            let mut mixer = Mixer::new(rate);
            let frames = rate as usize / 50;
            mixer
                .push(Source::System, &pcm(&vec![100; frames]))
                .unwrap();
            for _ in 0..4 {
                mixer
                    .push(Source::Microphone, &pcm(&vec![200; frames / 4]))
                    .unwrap();
            }
            assert_eq!(mixer.render(), pcm(&vec![300; frames]));
        }
    }

    #[test]
    fn delayed_inputs_are_bounded_and_malformed_packets_do_not_corrupt_queued_audio() {
        let mut mixer = Mixer::new(100);
        let input: Vec<i16> = (0..35).collect();
        mixer.push(Source::Microphone, &pcm(&input)).unwrap();
        assert_eq!(mixer.microphone.len(), 25);
        assert!(mixer.push(Source::Microphone, &[1]).is_err());
        assert_eq!(mixer.render(), pcm(&[10, 11]));
    }
    #[test]
    fn pause_boundaries_clear_both_sources_without_replaying_paused_audio() {
        let mut mixer = Mixer::new(100);
        mixer.push(Source::System, &pcm(&[100, 200])).unwrap();
        mixer.push(Source::Microphone, &pcm(&[300, 400])).unwrap();
        mixer.synchronize(2);
        assert_eq!(mixer.render(), pcm(&[0, 0]));
        mixer.push(Source::Microphone, &pcm(&[500, 600])).unwrap();
        mixer.synchronize(2);
        assert_eq!(mixer.render(), pcm(&[500, 600]));
    }
}
