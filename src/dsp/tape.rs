use super::biquad::Biquad;
use nih_plug::prelude::Enum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum TapeType {
    #[name = "Type I (Ferric Warm)"]
    Ferric,
    #[name = "Type II (Chrome Punch)"]
    Chrome,
    #[name = "Micro (Dictaphone Lo-Fi)"]
    MicroCassette,
}

pub struct TapeChannel {
    dc_x1: f32,
    dc_y1: f32,
    head_bump_filter: Biquad,
    hf_loss_filter: Biquad,
    sub_cut_filter: Biquad,
}

impl TapeChannel {
    pub fn new() -> Self {
        Self {
            dc_x1: 0.0,
            dc_y1: 0.0,
            head_bump_filter: Biquad::new(),
            hf_loss_filter: Biquad::new(),
            sub_cut_filter: Biquad::new(),
        }
    }

    pub fn reset(&mut self) {
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.head_bump_filter.reset();
        self.hf_loss_filter.reset();
        self.sub_cut_filter.reset();
    }

    #[inline(always)]
    fn dc_block(&mut self, x: f32) -> f32 {
        // Standard single-pole DC blocker (R ~ 0.998)
        let y = x - self.dc_x1 + 0.998 * self.dc_y1;
        self.dc_x1 = x;
        self.dc_y1 = y;
        y
    }
}

pub struct TapeProcessor {
    sample_rate: f32,
    channels: [TapeChannel; 2],
    last_tape_type: Option<TapeType>,
    last_head_bump: f32,
    last_age: f32,
}

impl TapeProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let mut proc = Self {
            sample_rate,
            channels: [TapeChannel::new(), TapeChannel::new()],
            last_tape_type: None,
            last_head_bump: f32::NAN,
            last_age: f32::NAN,
        };
        proc.reset(sample_rate);
        proc
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.last_tape_type = None;
        self.last_head_bump = f32::NAN;
        self.last_age = f32::NAN;
        for ch in &mut self.channels {
            ch.reset();
        }
    }

    pub fn update_filters_if_changed(
        &mut self,
        tape_type: TapeType,
        head_bump_db: f32,
        age_amount: f32,
    ) {
        if self.last_tape_type == Some(tape_type)
            && (self.last_head_bump - head_bump_db).abs() < 1e-4
            && (self.last_age - age_amount).abs() < 1e-4
        {
            return;
        }

        self.last_tape_type = Some(tape_type);
        self.last_head_bump = head_bump_db;
        self.last_age = age_amount;
        self.update_filters(tape_type, head_bump_db, age_amount);
    }

    pub fn update_filters(
        &mut self,
        tape_type: TapeType,
        head_bump_db: f32,
        age_amount: f32,
    ) {
        let (bump_freq, bump_q, base_hf_cutoff, sub_cutoff) = match tape_type {
            TapeType::Ferric => (95.0, 1.1, 11500.0, 30.0),
            TapeType::Chrome => (120.0, 1.3, 15500.0, 25.0),
            TapeType::MicroCassette => (170.0, 0.9, 5800.0, 180.0),
        };

        // Age scales HF cutoff downwards (older tape loses top-end aggressively)
        let hf_cutoff = (base_hf_cutoff * (1.0 - 0.55 * age_amount)).max(2500.0);

        // Effective head bump gain: base parameter + type characteristic
        let total_bump_db = match tape_type {
            TapeType::Ferric => head_bump_db + 2.0,
            TapeType::Chrome => head_bump_db + 0.5,
            TapeType::MicroCassette => head_bump_db + 4.5,
        };

        for ch in &mut self.channels {
            ch.head_bump_filter.set_peaking(
                self.sample_rate,
                bump_freq,
                bump_q,
                total_bump_db,
            );
            ch.hf_loss_filter.set_lowpass(
                self.sample_rate,
                hf_cutoff,
                0.707,
            );
            ch.sub_cut_filter.set_highpass(
                self.sample_rate,
                sub_cutoff,
                0.707,
            );
        }
    }

    #[inline(always)]
    pub fn process_stereo(
        &mut self,
        left: f32,
        right: f32,
        drive_linear: f32,
        bias: f32,
    ) -> (f32, f32) {
        let l_out = self.process_channel(0, left, drive_linear, bias);
        let r_out = self.process_channel(1, right, drive_linear, bias);
        (l_out, r_out)
    }

    #[inline(always)]
    pub fn saturate(x: f32) -> f32 {
        (5.0 / 3.0) * (0.6 * x).tanh()
    }

    #[inline(always)]
    fn process_channel(&mut self, ch_idx: usize, input: f32, drive: f32, bias: f32) -> f32 {
        let ch = &mut self.channels[ch_idx];

        // 1. Sub-bass rumble protection
        let x = ch.sub_cut_filter.process(input);

        // 2. Head bump resonance (pre-saturation contour)
        let x_bump = ch.head_bump_filter.process(x);

        // 3. Drive and Tape Bias Asymmetry
        // Bias shifts operating point adding warm 2nd order harmonics
        let driven = x_bump * drive;
        let biased = driven + 0.15 * bias * (driven * driven.abs());

        // 4. Asymmetric soft saturation curve (Tape hysteresis approximation)
        // High levels compress smoothly into warm tape saturation
        let sat = Self::saturate(biased);

        // 5. Remove DC offset generated by asymmetric bias
        let clean = ch.dc_block(sat);

        // 6. High frequency tape loss (gap loss + wear)
        ch.hf_loss_filter.process(clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_updates_if_changed() {
        let mut tape = TapeProcessor::new(48000.0);
        assert_eq!(tape.last_tape_type, None);

        tape.update_filters_if_changed(TapeType::Ferric, 3.5, 0.25);
        assert_eq!(tape.last_tape_type, Some(TapeType::Ferric));
        assert_eq!(tape.last_head_bump, 3.5);
        assert_eq!(tape.last_age, 0.25);

        // Calling with same parameters retains cached values
        tape.update_filters_if_changed(TapeType::Ferric, 3.5, 0.25);
        assert_eq!(tape.last_tape_type, Some(TapeType::Ferric));

        // Reset clears cache
        tape.reset(48000.0);
        assert_eq!(tape.last_tape_type, None);
    }

    #[test]
    fn test_saturation_invariants() {
        // Invariant 1: Small-signal slope at origin is 1.0
        let h = 1e-4f32;
        let slope_origin = (TapeProcessor::saturate(h) - TapeProcessor::saturate(-h)) / (2.0 * h);
        assert!(
            (slope_origin - 1.0).abs() < 1e-3,
            "Small signal slope at origin is {}, expected 1.0",
            slope_origin
        );

        // Invariant 2: Odd symmetry f(-x) = -f(x)
        let test_points = [0.1, 0.5, 0.999, 1.0, 1.001, 2.0, 5.0, 15.85];
        for &x in &test_points {
            let pos = TapeProcessor::saturate(x);
            let neg = TapeProcessor::saturate(-x);
            assert!(
                (pos + neg).abs() < 1e-5,
                "Odd symmetry violated at x={}: f(x)={}, f(-x)={}",
                x,
                pos,
                neg
            );
        }

        // Invariant 3: Monotonicity up to +24 dBFS (x approx 15.85)
        let mut prev_y = -2.0f32;
        let steps = 1000;
        for i in 0..=steps {
            let x = (i as f32 / steps as f32) * 16.0;
            let y = TapeProcessor::saturate(x);
            assert!(
                y >= prev_y,
                "Monotonicity violated at x={}: y={}, prev_y={}",
                x,
                y,
                prev_y
            );
            prev_y = y;
        }

        // Invariant 4: Finite and bounded output
        for &x in &[0.0, 1.0, 15.85, 100.0, 1000.0] {
            let y = TapeProcessor::saturate(x);
            assert!(y.is_finite(), "Output non finite at x={}", x);
            assert!(y <= 1.7, "Output exceeded bounded ceiling at x={}: y={}", x, y);
        }

        // Invariant 5: Derivative continuity across |x| in [0.999, 1.001]
        // Slope below 1.0:
        let slope_below = (TapeProcessor::saturate(0.9995 + h) - TapeProcessor::saturate(0.9995 - h)) / (2.0 * h);
        // Slope above 1.0:
        let slope_above = (TapeProcessor::saturate(1.0005 + h) - TapeProcessor::saturate(1.0005 - h)) / (2.0 * h);
        let slope_delta = (slope_above - slope_below).abs();
        assert!(
            slope_delta < 0.05,
            "Derivative discontinuity across |x|=1: slope_below={}, slope_above={}, delta={}",
            slope_below,
            slope_above,
            slope_delta
        );
    }
}
