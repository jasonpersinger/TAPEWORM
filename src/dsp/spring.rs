use super::biquad::Biquad;
use nih_plug::prelude::Enum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum SpringRouting {
    #[name = "Pre-Tape (Haunting / Fluttered)"]
    PreTape,
    #[name = "Post-Tape (Clean Space)"]
    PostTape,
}

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    damp_state: f32,
}

impl CombFilter {
    fn new(size_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; size_samples.max(16)],
            pos: 0,
            damp_state: 0.0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
        self.damp_state = 0.0;
    }

    #[inline(always)]
    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.pos];

        // 1-pole lowpass damping in feedback loop
        self.damp_state = output * (1.0 - damping) + self.damp_state * damping;

        self.buffer[self.pos] = input + self.damp_state * feedback;
        self.pos = (self.pos + 1) % self.buffer.len();

        output
    }
}

struct AllPassFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
}

impl AllPassFilter {
    fn new(size_samples: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; size_samples.max(16)],
            pos: 0,
            feedback,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
    }

    #[inline(always)]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buffer[self.pos];
        let new_buf = input + buf_out * self.feedback;
        let output = -input + new_buf * (1.0 - self.feedback * self.feedback);

        self.buffer[self.pos] = new_buf;
        self.pos = (self.pos + 1) % self.buffer.len();

        output
    }
}

pub struct DungeonSpring {
    sample_rate: f32,
    allpass_l: [AllPassFilter; 2],
    allpass_r: [AllPassFilter; 2],
    combs_l: [CombFilter; 4],
    combs_r: [CombFilter; 4],
    tone_filter_l: Biquad,
    tone_filter_r: Biquad,
}

impl DungeonSpring {
    pub fn new(sample_rate: f32) -> Self {
        let mut spring = Self {
            sample_rate,
            allpass_l: [
                AllPassFilter::new((0.0048 * sample_rate) as usize, 0.6),
                AllPassFilter::new((0.0017 * sample_rate) as usize, 0.6),
            ],
            allpass_r: [
                AllPassFilter::new((0.0051 * sample_rate) as usize, 0.6),
                AllPassFilter::new((0.0019 * sample_rate) as usize, 0.6),
            ],
            combs_l: [
                CombFilter::new((0.029 * sample_rate) as usize),
                CombFilter::new((0.034 * sample_rate) as usize),
                CombFilter::new((0.039 * sample_rate) as usize),
                CombFilter::new((0.045 * sample_rate) as usize),
            ],
            combs_r: [
                CombFilter::new((0.030 * sample_rate) as usize),
                CombFilter::new((0.036 * sample_rate) as usize),
                CombFilter::new((0.041 * sample_rate) as usize),
                CombFilter::new((0.047 * sample_rate) as usize),
            ],
            tone_filter_l: Biquad::new(),
            tone_filter_r: Biquad::new(),
        };
        spring.reset(sample_rate);
        spring
    }

    /// Re-allocates buffers for a new sample rate (only call during initialize, not during reset).
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if (self.sample_rate - sample_rate).abs() > 0.1 {
            *self = Self::new(sample_rate);
        } else {
            self.reset(sample_rate);
        }
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for ap in &mut self.allpass_l {
            ap.reset();
        }
        for ap in &mut self.allpass_r {
            ap.reset();
        }
        for c in &mut self.combs_l {
            c.reset();
        }
        for c in &mut self.combs_r {
            c.reset();
        }
        // Dark, resonant murky dungeon chamber tone
        self.tone_filter_l.reset();
        self.tone_filter_l.set_lowpass(sample_rate, 4200.0, 0.85);
        self.tone_filter_r.reset();
        self.tone_filter_r.set_lowpass(sample_rate, 4200.0, 0.85);
    }

    #[inline(always)]
    pub fn process_stereo(
        &mut self,
        left: f32,
        right: f32,
        amount: f32,
        decay: f32,
        damp: f32,
    ) -> (f32, f32) {
        let feedback = (0.75 + 0.22 * decay).clamp(0.6, 0.97);
        let damping = (0.2 + 0.65 * damp).clamp(0.1, 0.85);

        // Diffuse transients with allpass chain
        let mut diff_l = left;
        for ap in &mut self.allpass_l {
            diff_l = ap.process(diff_l);
        }

        let mut diff_r = right;
        for ap in &mut self.allpass_r {
            diff_r = ap.process(diff_r);
        }

        // Sum parallel comb filters for spring tank resonance
        let mut tank_l = 0.0;
        for c in &mut self.combs_l {
            tank_l += c.process(diff_l, feedback, damping);
        }

        let mut tank_r = 0.0;
        for c in &mut self.combs_r {
            tank_r += c.process(diff_r, feedback, damping);
        }

        // Normalize comb bank gain
        tank_l *= 0.25;
        tank_r *= 0.25;

        // Dark spring tone shaping
        let wet_l = self.tone_filter_l.process(tank_l) * amount;
        let wet_r = self.tone_filter_r.process(tank_r) * amount;

        (left + wet_l, right + wet_r)
    }

    #[cfg(test)]
    pub fn comb_lengths(&self) -> (Vec<usize>, Vec<usize>) {
        (
            self.combs_l.iter().map(|c| c.buffer.len()).collect(),
            self.combs_r.iter().map(|c| c.buffer.len()).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spring_rescaling_at_multiple_sample_rates() {
        let mut spring = DungeonSpring::new(44100.0);
        let (combs_l_44k, _) = spring.comb_lengths();
        assert_eq!(combs_l_44k[0], (0.029 * 44100.0) as usize); // 1278

        // Calibrate to 48 kHz
        spring.set_sample_rate(48000.0);
        let (combs_l_48k, _) = spring.comb_lengths();
        assert_eq!(combs_l_48k[0], (0.029 * 48000.0) as usize); // 1392

        // Calibrate to 96 kHz
        spring.set_sample_rate(96000.0);
        let (combs_l_96k, _) = spring.comb_lengths();
        assert_eq!(combs_l_96k[0], (0.029 * 96000.0) as usize); // 2784

        // Reset clears without resizing/reallocating
        spring.reset(96000.0);
        let (combs_l_reset, _) = spring.comb_lengths();
        assert_eq!(combs_l_reset[0], 2784);
    }

    #[test]
    fn test_spring_zero_level_advances_and_decays() {
        let mut spring = DungeonSpring::new(48000.0);

        // Put an impulse through with spring level on
        let _ = spring.process_stereo(1.0, 1.0, 0.5, 0.5, 0.5);

        // Turn spring level to zero during 5 seconds of silence (240,000 samples)
        for _ in 0..240000 {
            let (out_l, out_r) = spring.process_stereo(0.0, 0.0, 0.0, 0.5, 0.5);
            assert_eq!(out_l, 0.0);
            assert_eq!(out_r, 0.0);
        }

        // Turn spring level back on; old audio in the tank must have decayed to zero
        let (resumed_l, resumed_r) = spring.process_stereo(0.0, 0.0, 0.5, 0.5, 0.5);
        assert!(
            resumed_l.abs() < 1e-5 && resumed_r.abs() < 1e-5,
            "Resumed audio from tank was not decayed: l={}, r={}",
            resumed_l,
            resumed_r
        );
    }
}
