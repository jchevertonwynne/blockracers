//! A small software mixer standing in for the original's DirectSound buffers: every
//! voice has a volume, a pan and a playback rate, with the original's conversions from
//! its 0..1 scales to decibels.

use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use std::collections::HashMap;
use std::num::NonZero;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const OUTPUT_RATE: u32 = 44100;
/// Frames mixed at a time; gains glide to their new values over one block.
const BLOCK: usize = 256;

/// `ConvertVolumeToDirectSound`: full volume is 0 dB, zero is -30 dB, and anything
/// under this is silence.
const MIN_AUDIBLE_VOLUME: f32 = 0.005;
const VOLUME_RANGE_DB: f32 = 30.0;
/// `ConvertPanToDirectSound`: a full pan takes 25 dB off the far channel.
const PAN_RANGE_DB: f32 = 25.0;
/// DirectSound's limits on a buffer's frequency.
const FREQUENCY_RANGE: (f32, f32) = (100.0, 100_000.0);

/// Decoded sound: 16-bit frames, interleaved when stereo.
pub struct Clip {
    pub samples: Vec<i16>,
    pub channels: u16,
    pub rate: u32,
}

impl Clip {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

/// How a voice is to sound, on the original's scales.
#[derive(Clone, Copy, PartialEq)]
pub struct Tone {
    /// 0 to 1.
    pub volume: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    /// Multiple of the sound's own sample rate.
    pub pitch: f32,
}

impl Default for Tone {
    fn default() -> Self {
        Tone {
            volume: 1.0,
            pan: 0.0,
            pitch: 1.0,
        }
    }
}

impl Tone {
    /// Left and right amplitudes.
    fn gains(&self) -> [f32; 2] {
        if self.volume < MIN_AUDIBLE_VOLUME {
            return [0.0; 2];
        }
        let amplitude = |db: f32| 10f32.powf(db / 20.0);
        let level = amplitude((self.volume.min(1.0) - 1.0) * VOLUME_RANGE_DB);
        let side = |pan: f32| {
            if pan >= 1.0 {
                0.0
            } else {
                amplitude(-pan.max(0.0) * PAN_RANGE_DB)
            }
        };
        [level * side(self.pan), level * side(-self.pan)]
    }
}

struct Voice {
    clip: Arc<Clip>,
    /// Position in frames.
    at: f64,
    /// Frames of the clip per output frame.
    step: f64,
    gains: [f32; 2],
    target: [f32; 2],
    looped: bool,
}

impl Voice {
    fn set(&mut self, tone: Tone) {
        self.target = tone.gains();
        let frequency =
            (self.clip.rate as f32 * tone.pitch).clamp(FREQUENCY_RANGE.0, FREQUENCY_RANGE.1);
        self.step = (frequency / OUTPUT_RATE as f32) as f64;
    }

    /// Adds this voice to `out` (interleaved stereo). Returns false once it has finished.
    fn mix(&mut self, out: &mut [f32]) -> bool {
        let frames = self.clip.frames();
        if frames == 0 {
            return false;
        }
        let count = out.len() / 2;
        let from = self.gains;
        let stereo = self.clip.channels == 2;
        let sample = |frame: usize, channel: usize| {
            let index = if stereo { frame * 2 + channel } else { frame };
            self.clip.samples[index] as f32 / 32768.0
        };
        for (i, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            if self.at >= frames as f64 {
                if !self.looped {
                    return false;
                }
                self.at %= frames as f64;
            }
            let (index, blend) = (self.at as usize, self.at.fract() as f32);
            let next = if index + 1 < frames {
                index + 1
            } else if self.looped {
                0
            } else {
                index
            };
            let glide = (i + 1) as f32 / count as f32;
            for (channel, value) in frame.iter_mut().enumerate() {
                let gain = from[channel] + (self.target[channel] - from[channel]) * glide;
                *value +=
                    (sample(index, channel) * (1.0 - blend) + sample(next, channel) * blend) * gain;
            }
            self.at += self.step;
        }
        self.gains = self.target;
        true
    }
}

#[derive(Default)]
struct Voices {
    playing: HashMap<u32, Voice>,
    next: u32,
}

/// The game's side of the mixer.
#[derive(Resource, Clone, Default)]
pub struct Mixer(Arc<Mutex<Voices>>);

impl Mixer {
    /// Starts `clip` from its beginning and returns a handle to it.
    pub fn play(&self, clip: &Arc<Clip>, looped: bool, tone: Tone) -> u32 {
        let mut voices = self.0.lock().unwrap();
        voices.next += 1;
        let id = voices.next;
        let mut voice = Voice {
            clip: clip.clone(),
            at: 0.0,
            step: 1.0,
            gains: [0.0; 2],
            target: [0.0; 2],
            looped,
        };
        voice.set(tone);
        voice.gains = voice.target;
        voices.playing.insert(id, voice);
        id
    }

    pub fn set(&self, id: u32, tone: Tone) {
        if let Some(voice) = self.0.lock().unwrap().playing.get_mut(&id) {
            voice.set(tone);
        }
    }

    pub fn stop(&self, id: u32) {
        self.0.lock().unwrap().playing.remove(&id);
    }

    pub fn playing(&self, id: u32) -> bool {
        self.0.lock().unwrap().playing.contains_key(&id)
    }

    #[cfg(test)]
    pub fn voices(&self) -> usize {
        self.0.lock().unwrap().playing.len()
    }
}

/// The mixer as something Bevy can play: one endless stereo stream.
#[derive(Asset, TypePath, Clone)]
pub struct MixerOutput(pub Mixer);

pub struct MixerStream {
    mixer: Mixer,
    block: Vec<f32>,
    at: usize,
}

impl MixerStream {
    fn refill(&mut self) {
        self.block.clear();
        self.block.resize(BLOCK * 2, 0.0);
        let mut voices = self.mixer.0.lock().unwrap();
        voices.playing.retain(|_, voice| voice.mix(&mut self.block));
        drop(voices);
        for value in &mut self.block {
            *value = value.clamp(-1.0, 1.0);
        }
        self.at = 0;
    }
}

impl Iterator for MixerStream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.at >= self.block.len() {
            self.refill();
        }
        self.at += 1;
        Some(self.block[self.at - 1])
    }
}

impl Source for MixerStream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(OUTPUT_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for MixerOutput {
    type Decoder = MixerStream;

    fn decoder(&self) -> MixerStream {
        MixerStream {
            mixer: self.0.clone(),
            block: Vec::new(),
            at: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(samples: Vec<i16>, rate: u32) -> Arc<Clip> {
        Arc::new(Clip {
            samples,
            channels: 1,
            rate,
        })
    }

    #[test]
    fn volume_and_pan_follow_the_original_decibel_scales() {
        let gains = |volume, pan| {
            Tone {
                volume,
                pan,
                pitch: 1.0,
            }
            .gains()
        };
        assert_eq!(gains(1.0, 0.0), [1.0, 1.0]);
        assert_eq!(gains(0.004, 0.0), [0.0, 0.0]);
        // 0.7 is 9 dB down; nothing at all is 30 dB down, but never heard.
        assert!((gains(0.7, 0.0)[0] - 10f32.powf(-0.45)).abs() < 1e-5);
        // Panned right, the left channel drops and the right stays put.
        let [left, right] = gains(1.0, 0.7);
        assert!((left - 10f32.powf(-0.875)).abs() < 1e-5 && right == 1.0);
        assert_eq!(gains(1.0, -1.0), [1.0, 0.0]);
    }

    #[test]
    fn one_shots_finish_and_loops_wrap() {
        let mixer = Mixer::default();
        let mut stream = MixerOutput(mixer.clone()).decoder();
        let sound = clip(vec![16384; 441], 44100);
        let once = mixer.play(&sound, false, Tone::default());
        let looped = mixer.play(
            &sound,
            true,
            Tone {
                pitch: 0.5,
                ..default()
            },
        );
        let heard: Vec<f32> = stream.by_ref().take(BLOCK * 2 * 4).collect();
        assert!(heard[0] > 0.9 && heard[BLOCK * 2 * 4 - 1] > 0.4 && heard[BLOCK * 2 * 4 - 1] < 0.6);
        assert!(!mixer.playing(once) && mixer.playing(looped));
        mixer.stop(looped);
        assert_eq!(mixer.voices(), 0);
    }
}
