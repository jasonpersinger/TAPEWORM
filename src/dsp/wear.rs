use super::biquad::Biquad;
use std::f32::consts::PI;

pub struct WearProcessor {
    sample_rate: f32,

    // Biquads for generation loss
    gen_l: [Biquad; 3],
    gen_r: [Biquad; 3],
    last_generation: u8,
    target_passes: f32,
    current_passes: f32,
    target_cutoff: f32,
    current_cutoff: f32,
    active_cutoff: f32,
    smooth_coeff: f32,
    sample_count: usize,

    // Dropout state machines (L & R)
    dropout_l_active: bool,
    dropout_l_samples_left: usize,
    dropout_l_gain: f32,
    dropout_l_target_gain: f32,
    dropout_l_recovery_gain: f32,
    dropout_l_filter: Biquad,

    dropout_r_active: bool,
    dropout_r_samples_left: usize,
    dropout_r_gain: f32,
    dropout_r_target_gain: f32,
    dropout_r_recovery_gain: f32,
    dropout_r_filter: Biquad,

    // Noise & Hum
    noise_filter_l: Biquad,
    noise_filter_r: Biquad,
    hum_phase: f32,
    rng_state: u32,

    // Dynamic noise envelope detector (compander breathing)
    music_env: f32,

    // Click-free noise gate with hysteresis (Option B)
    gate_open: bool,
    gate_env: f32,
    gate_attack: f32,
    gate_release: f32,
}

impl WearProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let mut wear = Self {
            sample_rate,
            gen_l: [Biquad::new(), Biquad::new(), Biquad::new()],
            gen_r: [Biquad::new(), Biquad::new(), Biquad::new()],
            last_generation: 0,
            target_passes: 0.0,
            current_passes: 0.0,
            target_cutoff: 12000.0,
            current_cutoff: 12000.0,
            active_cutoff: 12000.0,
            smooth_coeff: 0.0,
            sample_count: 0,

            dropout_l_active: false,
            dropout_l_samples_left: 0,
            dropout_l_gain: 1.0,
            dropout_l_target_gain: 1.0,
            dropout_l_recovery_gain: 0.0,
            dropout_l_filter: Biquad::new(),

            dropout_r_active: false,
            dropout_r_samples_left: 0,
            dropout_r_gain: 1.0,
            dropout_r_target_gain: 1.0,
            dropout_r_recovery_gain: 0.0,
            dropout_r_filter: Biquad::new(),

            noise_filter_l: Biquad::new(),
            noise_filter_r: Biquad::new(),
            hum_phase: 0.0,
            rng_state: 987654321,
            music_env: 0.0,

            gate_open: false,
            gate_env: 0.0,
            gate_attack: 0.0,
            gate_release: 0.0,
        };
        wear.reset(sample_rate);
        wear
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.last_generation = 0;
        self.target_passes = 0.0;
        self.current_passes = 0.0;
        self.target_cutoff = 12000.0;
        self.current_cutoff = 12000.0;
        self.active_cutoff = 12000.0;
        self.smooth_coeff = 1.0 - (-1.0 / (0.015 * sample_rate)).exp();
        self.sample_count = 0;
        for f in &mut self.gen_l {
            f.reset();
            f.set_lowpass(sample_rate, 12000.0, 0.65);
        }
        for f in &mut self.gen_r {
            f.reset();
            f.set_lowpass(sample_rate, 12000.0, 0.65);
        }
        self.dropout_l_active = false;
        self.dropout_l_gain = 1.0;
        self.dropout_l_target_gain = 1.0;
        self.dropout_l_recovery_gain = 0.0;
        self.dropout_l_filter.reset();
        self.dropout_l_filter.set_lowpass(sample_rate, 18000.0, 0.707);

        self.dropout_r_active = false;
        self.dropout_r_gain = 1.0;
        self.dropout_r_target_gain = 1.0;
        self.dropout_r_recovery_gain = 0.0;
        self.dropout_r_filter.reset();
        self.dropout_r_filter.set_lowpass(sample_rate, 18000.0, 0.707);

        // Noise filter: warm ferric tape noise bandpass
        self.noise_filter_l.reset();
        self.noise_filter_l.set_lowpass(sample_rate, 8500.0, 0.5);
        self.noise_filter_r.reset();
        self.noise_filter_r.set_lowpass(sample_rate, 8500.0, 0.5);

        self.hum_phase = 0.0;
        self.music_env = 0.0;

        self.gate_open = false;
        self.gate_env = 0.0;
        self.gate_attack = 1.0 - (-1.0 / (0.005 * sample_rate)).exp(); // ~5 ms attack
        self.gate_release = 1.0 - (-1.0 / (0.150 * sample_rate)).exp(); // ~150 ms release
    }

    pub fn gate_env(&self) -> f32 {
        self.gate_env
    }

    #[inline(always)]
    fn next_rand(&mut self) -> f32 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 17;
        self.rng_state ^= self.rng_state << 5;
        (self.rng_state as f32 / 2147483648.0) - 1.0
    }

    pub fn update_generation_filters(&mut self, generation: u8) {
        let clamped = generation.clamp(1, 4);
        let bounces = (clamped - 1) as f32;
        self.target_passes = bounces;
        self.target_cutoff = (12000.0 - bounces * 2200.0).max(3500.0);
        self.last_generation = clamped;
    }

    #[inline(always)]
    pub fn filter_noise_stereo(&mut self, raw_l: f32, raw_r: f32) -> (f32, f32) {
        let fn_l = self.noise_filter_l.process(raw_l);
        let fn_r = self.noise_filter_r.process(raw_r);
        (fn_l, fn_r)
    }

    #[inline(always)]
    pub fn process_stereo(
        &mut self,
        left: f32,
        right: f32,
        generation: u8,
        dropouts_amount: f32,
        hiss_level: f32,
        hum_level: f32,
        hum_freq_hz: f32,
    ) -> (f32, f32) {
        // 1. Generation Dub Cascades with continuous fractional tap crossfade
        let clamped = generation.clamp(1, 4);
        let bounces = (clamped - 1) as f32;
        if bounces != self.target_passes {
            self.update_generation_filters(clamped);
        }

        let (mut l, mut r) = if self.target_passes == 0.0 && left == 0.0 && right == 0.0 {
            // Clean digital silence at Gen 1 (bypassed dub loss)
            self.current_passes = 0.0;
            self.current_cutoff = self.target_cutoff;
            for i in 0..3 {
                self.gen_l[i].reset();
                self.gen_r[i].reset();
            }
            (0.0, 0.0)
        } else {
            // Smooth glide toward target passes and cutoff
            self.current_passes += self.smooth_coeff * (self.target_passes - self.current_passes);
            if (self.target_passes - self.current_passes).abs() < 1e-5 {
                self.current_passes = self.target_passes;
            }

            self.current_cutoff += self.smooth_coeff * (self.target_cutoff - self.current_cutoff);
            if (self.target_cutoff - self.current_cutoff).abs() < 1.0 {
                self.current_cutoff = self.target_cutoff;
            }

            if self.sample_count % 16 == 0 && (self.current_cutoff - self.active_cutoff).abs() > 0.5 {
                self.active_cutoff = self.current_cutoff;
                for i in 0..3 {
                    self.gen_l[i].set_lowpass(self.sample_rate, self.active_cutoff, 0.65);
                    self.gen_r[i].set_lowpass(self.sample_rate, self.active_cutoff, 0.65);
                }
            }
            self.sample_count += 1;

            let tap0_l = left;
            let tap0_r = right;

            let tap1_l = self.gen_l[0].process(tap0_l);
            let tap1_r = self.gen_r[0].process(tap0_r);

            let tap2_l = self.gen_l[1].process(tap1_l);
            let tap2_r = self.gen_r[1].process(tap1_r);

            let tap3_l = self.gen_l[2].process(tap2_l);
            let tap3_r = self.gen_r[2].process(tap2_r);

            let p = self.current_passes.clamp(0.0, 3.0);
            if p <= 1.0 {
                let w1 = p;
                let w0 = 1.0 - w1;
                (w0 * tap0_l + w1 * tap1_l, w0 * tap0_r + w1 * tap1_r)
            } else if p <= 2.0 {
                let w2 = p - 1.0;
                let w1 = 1.0 - w2;
                (w1 * tap1_l + w2 * tap2_l, w1 * tap1_r + w2 * tap2_r)
            } else {
                let w3 = p - 2.0;
                let w2 = 1.0 - w3;
                (w2 * tap2_l + w3 * tap3_l, w2 * tap2_r + w3 * tap3_r)
            }
        };

        // 2. Tape Crinkle & Dropouts (Random physical head-lift events)
        if dropouts_amount > 0.001 {
            // Chance per sample to trigger a dropout
            let trigger_prob = 0.00008 * dropouts_amount;

            // Left dropout trigger
            if !self.dropout_l_active && (self.next_rand() + 1.0) * 0.5 < trigger_prob {
                self.dropout_l_active = true;
                let length_ms = 40.0 + (self.next_rand() + 1.0) * 80.0; // 40-200ms
                self.dropout_l_samples_left = ((length_ms / 1000.0) * self.sample_rate) as usize;
                self.dropout_l_target_gain = 0.15 + (self.next_rand() + 1.0) * 0.2; // -8dB to -16dB
                self.dropout_l_recovery_gain = self.dropout_l_target_gain;
                self.dropout_l_filter.set_lowpass(self.sample_rate, 2200.0, 0.707);
            }

            // Right dropout trigger (independent)
            if !self.dropout_r_active && (self.next_rand() + 1.0) * 0.5 < trigger_prob {
                self.dropout_r_active = true;
                let length_ms = 40.0 + (self.next_rand() + 1.0) * 80.0;
                self.dropout_r_samples_left = ((length_ms / 1000.0) * self.sample_rate) as usize;
                self.dropout_r_target_gain = 0.15 + (self.next_rand() + 1.0) * 0.2;
                self.dropout_r_recovery_gain = self.dropout_r_target_gain;
                self.dropout_r_filter.set_lowpass(self.sample_rate, 2200.0, 0.707);
            }

            // Process Left Dropout
            if self.dropout_l_active {
                self.dropout_l_gain += 0.02 * (self.dropout_l_target_gain - self.dropout_l_gain);
                let filtered = self.dropout_l_filter.process(l);
                if self.dropout_l_samples_left > 0 {
                    self.dropout_l_samples_left -= 1;
                    l = filtered * self.dropout_l_gain;
                    if self.dropout_l_samples_left == 0 {
                        self.dropout_l_target_gain = 1.0;
                        self.dropout_l_recovery_gain = self.dropout_l_gain;
                    }
                } else {
                    let blend = ((self.dropout_l_gain - self.dropout_l_recovery_gain)
                        / (1.0 - self.dropout_l_recovery_gain).max(0.001))
                        .clamp(0.0, 1.0);
                    let wet = filtered * self.dropout_l_gain;
                    l = wet * (1.0 - blend) + l * blend;

                    if self.dropout_l_gain >= 0.995 || blend >= 0.999 {
                        self.dropout_l_active = false;
                        self.dropout_l_filter.set_lowpass(self.sample_rate, 18000.0, 0.707);
                    }
                }
            }

            // Process Right Dropout
            if self.dropout_r_active {
                self.dropout_r_gain += 0.02 * (self.dropout_r_target_gain - self.dropout_r_gain);
                let filtered = self.dropout_r_filter.process(r);
                if self.dropout_r_samples_left > 0 {
                    self.dropout_r_samples_left -= 1;
                    r = filtered * self.dropout_r_gain;
                    if self.dropout_r_samples_left == 0 {
                        self.dropout_r_target_gain = 1.0;
                        self.dropout_r_recovery_gain = self.dropout_r_gain;
                    }
                } else {
                    let blend = ((self.dropout_r_gain - self.dropout_r_recovery_gain)
                        / (1.0 - self.dropout_r_recovery_gain).max(0.001))
                        .clamp(0.0, 1.0);
                    let wet = filtered * self.dropout_r_gain;
                    r = wet * (1.0 - blend) + r * blend;

                    if self.dropout_r_gain >= 0.995 || blend >= 0.999 {
                        self.dropout_r_active = false;
                        self.dropout_r_filter.set_lowpass(self.sample_rate, 18000.0, 0.707);
                    }
                }
            }
        }

        // 3. Compander / Noise Floor Breathing & Gate
        let signal_level = 0.5 * (l.abs() + r.abs());
        let env_attack = 0.01;
        let env_release = 0.0005;
        if signal_level > self.music_env {
            self.music_env += env_attack * (signal_level - self.music_env);
        } else {
            self.music_env += env_release * (signal_level - self.music_env);
        }

        // Noise gate with hysteresis: open at -70 dBFS (0.000316), close at -78 dBFS (0.000125)
        if self.music_env > 0.000316 {
            self.gate_open = true;
        } else if self.music_env < 0.000125 {
            self.gate_open = false;
        }

        // Smooth gate envelope (5 ms attack, 150 ms release)
        let gate_target = if self.gate_open { 1.0 } else { 0.0 };
        if gate_target > self.gate_env {
            self.gate_env += self.gate_attack * (gate_target - self.gate_env);
        } else {
            self.gate_env += self.gate_release * (gate_target - self.gate_env);
            if self.gate_env < 1e-6 {
                self.gate_env = 0.0;
            }
        }

        // Slight ducking of tape noise when loud music plays, breathing up during quiet passages
        let noise_mult = (1.0 - 0.45 * (self.music_env * 3.0).min(1.0)).max(0.2);

        // 4. Hiss generation (ferric bandpass noise) - strictly zero when turned down or fully gated
        let (filtered_noise_l, filtered_noise_r) = if hiss_level > 0.0005 && self.gate_env > 0.0 {
            let raw_noise_l = self.next_rand();
            let raw_noise_r = self.next_rand();
            let (fn_l, fn_r) = self.filter_noise_stereo(raw_noise_l, raw_noise_r);
            let mult = hiss_level * 0.04 * noise_mult * self.gate_env;
            (fn_l * mult, fn_r * mult)
        } else {
            (0.0, 0.0)
        };

        // 5. 50/60 Hz Transformer Ground Hum with Harmonics - oscillator runs continuously, scaled by smooth gate
        let full_hum = if hum_level > 0.0005 {
            let hum_inc = (2.0 * PI * hum_freq_hz) / self.sample_rate;
            self.hum_phase += hum_inc;
            if self.hum_phase >= 2.0 * PI {
                self.hum_phase -= 2.0 * PI;
            }
            if self.gate_env > 0.0 {
                let hum_fundamental = self.hum_phase.sin();
                let hum_second = (self.hum_phase * 2.0).sin() * 0.45;
                let hum_third = (self.hum_phase * 3.0).sin() * 0.25;
                (hum_fundamental + hum_second + hum_third) * hum_level * 0.025 * self.gate_env
            } else {
                0.0
            }
        } else {
            0.0
        };

        (l + filtered_noise_l + full_hum, r + filtered_noise_r + full_hum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generation_switch_no_stale_burst() {
        let mut wear = WearProcessor::new(48000.0);

        // 1. Process audio at generation 2
        wear.update_generation_filters(2);
        for i in 0..1000 {
            let input = ((i as f32) * 0.1).sin();
            let _ = wear.process_stereo(input, input, 2, 0.0, 0.0, 0.0, 60.0);
        }

        // 2. Switch to generation 1 during silence
        wear.update_generation_filters(1);
        for _ in 0..1000 {
            let (out_l, out_r) = wear.process_stereo(0.0, 0.0, 1, 0.0, 0.0, 0.0, 60.0);
            assert_eq!(out_l, 0.0);
            assert_eq!(out_r, 0.0);
        }

        // 3. Switch back to generation 2 while still silent
        wear.update_generation_filters(2);
        for _ in 0..100 {
            let (out_l, out_r) = wear.process_stereo(0.0, 0.0, 2, 0.0, 0.0, 0.0, 60.0);
            // Stale history must not cause a burst (was ~0.027 before the fix)
            assert!(
                out_l.abs() < 1e-6 && out_r.abs() < 1e-6,
                "Stale filter history burst detected: l={}, r={}",
                out_l,
                out_r
            );
        }
    }

    #[test]
    fn test_dropout_recovery_smooth_crossfade() {
        let mut wear = WearProcessor::new(48000.0);
        // Force trigger a left dropout
        wear.dropout_l_active = true;
        wear.dropout_l_samples_left = 50;
        wear.dropout_l_target_gain = 0.2;
        wear.dropout_l_recovery_gain = 0.2;
        wear.dropout_l_filter.set_lowpass(48000.0, 2200.0, 0.707);

        // Play an 8 kHz sine wave through dropout and observe transition across deactivation
        let mut max_transition_delta: f32 = 0.0;
        let mut prev_sample: Option<f32> = None;
        let mut was_active = true;

        for i in 0..2000 {
            let phase = 2.0 * PI * 8000.0 * (i as f32) / 48000.0;
            let input = phase.sin();
            let (out_l, _) = wear.process_stereo(input, 0.0, 1, 0.5, 0.0, 0.0, 60.0);

            if was_active && !wear.dropout_l_active {
                // This sample is the exact moment dropout was disabled!
                if let Some(prev) = prev_sample {
                    let expected_dry_delta = (input - (2.0 * PI * 8000.0 * ((i - 1) as f32) / 48000.0).sin()).abs();
                    let actual_delta = (out_l - prev).abs();
                    let transition_step = (actual_delta - expected_dry_delta).abs();
                    if transition_step > max_transition_delta {
                        max_transition_delta = transition_step;
                    }
                }
                was_active = false;
            }
            prev_sample = Some(out_l);
        }

        assert!(
            max_transition_delta < 0.05,
            "Dropout recovery step discontinuity detected: delta was {}",
            max_transition_delta
        );
    }

    #[test]
    fn test_noise_filter_channel_independence() {
        let sample_rate = 48000.0;
        let mut wear = WearProcessor::new(sample_rate);

        let mut ref_l = Biquad::new();
        ref_l.set_lowpass(sample_rate, 8500.0, 0.5);

        let mut ref_r = Biquad::new();
        ref_r.set_lowpass(sample_rate, 8500.0, 0.5);

        for i in 0..100 {
            let in_l = ((i as f32) * 0.1).sin();
            let in_r = 0.0f32;

            let expected_l = ref_l.process(in_l);
            let expected_r = ref_r.process(in_r);

            let (actual_l, actual_r) = wear.filter_noise_stereo(in_l, in_r);

            assert!(
                (actual_l - expected_l).abs() < 1e-6,
                "Left noise channel contaminated at sample {}: actual={}, expected={}",
                i, actual_l, expected_l
            );
            assert!(
                (actual_r - expected_r).abs() < 1e-6,
                "Right noise channel contaminated by Left history at sample {}: actual={}, expected={}",
                i, actual_r, expected_r
            );
        }
    }

    #[test]
    fn test_silence_input_produces_zero_noise_when_hiss_and_hum_active() {
        let mut wear = WearProcessor::new(48000.0);
        // Feed silence with hiss and hum maxed
        for _ in 0..1000 {
            let (out_l, out_r) = wear.process_stereo(0.0, 0.0, 1, 0.0, 1.0, 1.0, 60.0);
            assert_eq!(out_l, 0.0);
            assert_eq!(out_r, 0.0);
        }
    }

    #[test]
    fn test_gate_threshold_crossing_discontinuity() {
        let mut wear = WearProcessor::new(48000.0);
        // Prime the processor with a tone above threshold (-60 dBFS)
        for _ in 0..1000 {
            let _ = wear.process_stereo(0.001, 0.001, 1, 0.0, 0.0, 1.0, 60.0);
        }
        assert!(wear.gate_env() > 0.9, "Gate should be fully open above threshold");

        // Fade down to zero over 10000 samples
        let mut max_envelope_step = 0.0f32;
        let mut prev_env = wear.gate_env();

        for i in 0..10000 {
            let in_val = if i < 2000 {
                0.001 * (1.0 - (i as f32) / 2000.0)
            } else {
                0.0
            };
            let _ = wear.process_stereo(in_val, in_val, 1, 0.0, 0.0, 1.0, 60.0);
            let current_env = wear.gate_env();
            let step = (current_env - prev_env).abs();
            if step > max_envelope_step {
                max_envelope_step = step;
            }
            prev_env = current_env;
        }

        // In a click-free smooth gate, the gate envelope transition per sample
        // should be smoothly governed by release time (~150 ms => ~0.00014 per sample)
        // rather than jumping abruptly (the old hard cutoff jumped by > 0.001 in a single sample).
        assert!(
            max_envelope_step < 0.0005,
            "Gate envelope abrupt cutoff step detected: max step was {}",
            max_envelope_step
        );
    }

    #[test]
    fn test_generation_switch_click_free_continuous_audio() {
        let sample_rate = 48000.0;
        let mut wear = WearProcessor::new(sample_rate);

        let transitions = [
            (1u8, 2u8),
            (2u8, 3u8),
            (3u8, 4u8),
            (4u8, 1u8),
            (1u8, 4u8),
            (4u8, 3u8),
            (3u8, 2u8),
            (2u8, 1u8),
        ];

        // Test with 1 kHz sine at 0.8 amplitude
        for &(from_g, to_g) in &transitions {
            wear.reset(sample_rate);
            wear.update_generation_filters(from_g);

            // Settle initial generation
            for i in 0..4800 {
                let s = (2.0 * PI * 1000.0 * (i as f32) / sample_rate).sin() * 0.8;
                let _ = wear.process_stereo(s, s, from_g, 0.0, 0.0, 0.0, 60.0);
            }

            // Steady pre-switch run
            let mut out_l = Vec::new();
            for i in 0..2400 {
                let s = (2.0 * PI * 1000.0 * ((i + 4800) as f32) / sample_rate).sin() * 0.8;
                let (ol, _) = wear.process_stereo(s, s, from_g, 0.0, 0.0, 0.0, 60.0);
                out_l.push(ol);
            }

            // Switch generation
            wear.update_generation_filters(to_g);

            // Post-switch run
            for i in 2400..7200 {
                let s = (2.0 * PI * 1000.0 * ((i + 4800) as f32) / sample_rate).sin() * 0.8;
                let (ol, _) = wear.process_stereo(s, s, to_g, 0.0, 0.0, 0.0, 60.0);
                out_l.push(ol);
            }

            // Verify no step discontinuity across transition window (+/- 1000 samples)
            let mut max_step = 0.0f32;
            for i in 2395..3400 {
                let jump = (out_l[i] - out_l[i - 1]).abs();
                let prev_jump = (out_l[i - 1] - out_l[i - 2]).abs();
                let step = (jump - prev_jump).max(0.0);
                if step > max_step {
                    max_step = step;
                }
            }

            assert!(
                max_step < 0.025,
                "Transition {} -> {} caused audible click: max step = {:.6} (limit = 0.025)",
                from_g,
                to_g,
                max_step
            );
        }
    }

    #[test]
    fn test_generation_rapid_switching_stability() {
        let sample_rate = 48000.0;
        let mut wear = WearProcessor::new(sample_rate);
        let gens = [1u8, 2, 3, 4, 1, 4, 3, 2, 1, 3, 1, 4, 2, 1];

        // Rapid switching every 64 samples
        let mut out_l = Vec::new();
        let mut sample_idx = 0;

        for &g in &gens {
            wear.update_generation_filters(g);
            for _ in 0..64 {
                let s = (2.0 * PI * 1000.0 * (sample_idx as f32) / sample_rate).sin() * 0.8;
                let (ol, or) = wear.process_stereo(s, s, g, 0.0, 0.0, 0.0, 60.0);
                assert!(ol.is_finite() && or.is_finite(), "Output must be finite during rapid switching");
                out_l.push(ol);
                sample_idx += 1;
            }
        }

        let mut max_step = 0.0f32;
        for i in 2..out_l.len() {
            let delta = (out_l[i] - out_l[i - 1]).abs();
            let prev_delta = (out_l[i - 1] - out_l[i - 2]).abs();
            let step = (delta - prev_delta).max(0.0);
            if step > max_step {
                max_step = step;
            }
        }

        assert!(
            max_step < 0.025,
            "Rapid generation switching caused step discontinuity: {:.6}",
            max_step
        );
    }

    #[test]
    fn test_generation_switch_across_sample_rates() {
        let sample_rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];

        for &sr in &sample_rates {
            let mut wear = WearProcessor::new(sr);
            wear.update_generation_filters(1);

            let mut out_l = Vec::new();
            for i in 0..1000 {
                let s = (2.0 * PI * 1000.0 * (i as f32) / sr).sin() * 0.8;
                let (ol, _) = wear.process_stereo(s, s, 1, 0.0, 0.0, 0.0, 60.0);
                out_l.push(ol);
            }

            wear.update_generation_filters(4);

            for i in 1000..3000 {
                let s = (2.0 * PI * 1000.0 * (i as f32) / sr).sin() * 0.8;
                let (ol, or) = wear.process_stereo(s, s, 4, 0.0, 0.0, 0.0, 60.0);
                assert!(ol.is_finite() && or.is_finite());
                out_l.push(ol);
            }

            let mut max_step = 0.0f32;
            for i in 995..1500 {
                let d = (out_l[i] - out_l[i - 1]).abs();
                let prev_d = (out_l[i - 1] - out_l[i - 2]).abs();
                let step = (d - prev_d).max(0.0);
                if step > max_step {
                    max_step = step;
                }
            }

            assert!(
                max_step < 0.025,
                "Sample rate {} Hz switch 1 -> 4 produced step {:.6}",
                sr,
                max_step
            );
        }
    }
}
