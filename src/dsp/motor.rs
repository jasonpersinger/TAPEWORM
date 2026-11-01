use std::f32::consts::PI;

const MAX_DELAY_SAMPLES: usize = 4096;
const REFERENCE_SAMPLE_RATE: f32 = 44100.0;
const WOW_EXCURSION_SEC: f32 = 14.0 / REFERENCE_SAMPLE_RATE;
const FLUTTER_EXCURSION_SEC: f32 = 4.5 / REFERENCE_SAMPLE_RATE;
const PINCH_EXCURSION_SEC: f32 = 8.0 / REFERENCE_SAMPLE_RATE;
const BOG_EXCURSION_SEC: f32 = 18.0 / REFERENCE_SAMPLE_RATE;
const AZIMUTH_EXCURSION_SEC: f32 = 4.0 / REFERENCE_SAMPLE_RATE;

pub struct MotorProcessor {
    sample_rate: f32,
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    nominal_delay_samples: usize,

    // LFO phases
    wow_phase: f32,
    wow_phase2: f32,
    wow_rate: f32,
    flutter_phase: f32,
    flutter_rate: f32,
    pinch_phase: f32,
    azimuth_phase: f32,

    // Motor Bog envelope detector (transient pitch sag)
    bog_env: f32,
    bog_smoothed: f32,

    // Pseudo-random noise state for organic jitter
    rng_state: u32,
}

impl MotorProcessor {
    pub fn calc_nominal_delay_samples(sample_rate: f32) -> usize {
        ((0.015 * sample_rate).round() as usize).clamp(100, MAX_DELAY_SAMPLES - 100)
    }

    pub fn nominal_delay_samples(&self) -> usize {
        self.nominal_delay_samples
    }

    pub fn new(sample_rate: f32) -> Self {
        let nominal_delay_samples = Self::calc_nominal_delay_samples(sample_rate);
        Self {
            sample_rate,
            buffer_l: vec![0.0; MAX_DELAY_SAMPLES],
            buffer_r: vec![0.0; MAX_DELAY_SAMPLES],
            write_pos: 0,
            nominal_delay_samples,

            wow_phase: 0.0,
            wow_phase2: 0.0,
            wow_rate: 0.8, // 0.8 Hz slow wow
            flutter_phase: 0.0,
            flutter_rate: 15.0, // 15 Hz motor flutter
            pinch_phase: 0.0,
            azimuth_phase: 0.0,

            bog_env: 0.0,
            bog_smoothed: 0.0,
            rng_state: 123456789,
        }
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.nominal_delay_samples = Self::calc_nominal_delay_samples(sample_rate);
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
        self.wow_phase = 0.0;
        self.wow_phase2 = 0.0;
        self.flutter_phase = 0.0;
        self.pinch_phase = 0.0;
        self.azimuth_phase = 0.0;
        self.bog_env = 0.0;
        self.bog_smoothed = 0.0;
    }

    #[inline(always)]
    fn next_rand(&mut self) -> f32 {
        // Simple Xorshift RNG producing -1.0..1.0
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 17;
        self.rng_state ^= self.rng_state << 5;
        (self.rng_state as f32 / 2147483648.0) - 1.0
    }

    #[inline(always)]
    pub fn compute_delays(
        &mut self,
        input_energy: f32,
        wow_depth: f32,
        flutter_depth: f32,
        bog_depth: f32,
        pinch_depth: f32,
        azimuth_depth: f32,
    ) -> (f32, f32) {
        let nominal_delay = self.nominal_delay_samples as f32;

        // 1. Motor Bog: Track transient level to sag capstan motor
        let attack_coeff = (-1.0 / (0.008 * self.sample_rate)).exp(); // ~8 ms attack
        let release_coeff = (-1.0 / (0.220 * self.sample_rate)).exp(); // ~220 ms recovery

        // Smooth saturation of input energy prevents envelope runaway and buffer overrun
        // on extreme signals (> +12 dBFS) while remaining virtually linear for nominal audio
        let safe_input = if input_energy.is_finite() && input_energy > 0.0 {
            2.0 * (0.5 * input_energy).tanh()
        } else {
            0.0
        };

        if safe_input > self.bog_env {
            self.bog_env = safe_input + attack_coeff * (self.bog_env - safe_input);
        } else {
            self.bog_env = safe_input + release_coeff * (self.bog_env - safe_input);
        }

        // Smooth the bog pitch dip
        self.bog_smoothed += 0.05 * (self.bog_env - self.bog_smoothed);
        let bog_delay_samples = self.bog_smoothed * bog_depth * (BOG_EXCURSION_SEC * self.sample_rate);

        // 2. Wow LFO (Slow capstan / reel speed variance)
        let wow_inc = (2.0 * PI * self.wow_rate) / self.sample_rate;
        self.wow_phase += wow_inc;
        if self.wow_phase >= 2.0 * PI {
            self.wow_phase -= 2.0 * PI;
        }
        let wow_inc2 = (2.0 * PI * self.wow_rate * 2.3) / self.sample_rate;
        self.wow_phase2 += wow_inc2;
        if self.wow_phase2 >= 2.0 * PI {
            self.wow_phase2 -= 2.0 * PI;
        }
        let wow_mod = (self.wow_phase.sin() + 0.3 * self.wow_phase2.sin())
            * wow_depth
            * (WOW_EXCURSION_SEC * self.sample_rate);

        // 3. Flutter LFO (Fast mechanical scrape / belt jitter)
        let flutter_inc = (2.0 * PI * self.flutter_rate) / self.sample_rate;
        self.flutter_phase += flutter_inc;
        if self.flutter_phase >= 2.0 * PI {
            self.flutter_phase -= 2.0 * PI;
        }
        let jitter = self.next_rand() * 0.15;
        let flutter_mod = (self.flutter_phase.sin() + jitter)
            * flutter_depth
            * (FLUTTER_EXCURSION_SEC * self.sample_rate);

        // 4. Pinch Roller Bump (Periodic thud every ~0.9s)
        let pinch_inc = (2.0 * PI * 1.1) / self.sample_rate;
        self.pinch_phase += pinch_inc;
        if self.pinch_phase >= 2.0 * PI {
            self.pinch_phase -= 2.0 * PI;
        }
        // Narrow bump pulse
        let pinch_pulse = (self.pinch_phase.sin()).max(0.0).powi(12);
        let pinch_mod = pinch_pulse * pinch_depth * (PINCH_EXCURSION_SEC * self.sample_rate);

        // 5. Azimuth Drift (L/R phase skew)
        let az_inc = (2.0 * PI * 0.25) / self.sample_rate; // Very slow wander (0.25 Hz)
        self.azimuth_phase += az_inc;
        if self.azimuth_phase >= 2.0 * PI {
            self.azimuth_phase -= 2.0 * PI;
        }
        let az_mod = self.azimuth_phase.sin() * azimuth_depth * (AZIMUTH_EXCURSION_SEC * self.sample_rate);

        // Total delay for Left and Right channels
        let base_delay = nominal_delay + wow_mod + flutter_mod + pinch_mod + bog_delay_samples;
        let delay_l = (base_delay + az_mod).clamp(4.0, (MAX_DELAY_SAMPLES - 5) as f32);
        let delay_r = (base_delay - az_mod).clamp(4.0, (MAX_DELAY_SAMPLES - 5) as f32);

        (delay_l, delay_r)
    }

    #[inline(always)]
    pub fn process_stereo(
        &mut self,
        left: f32,
        right: f32,
        wow_depth: f32,
        flutter_depth: f32,
        bog_depth: f32,
        pinch_depth: f32,
        azimuth_depth: f32,
    ) -> (f32, f32) {
        // Write incoming audio to circular buffers
        self.buffer_l[self.write_pos] = left;
        self.buffer_r[self.write_pos] = right;

        let input_energy = 0.5 * (left.abs() + right.abs());
        let (delay_l, delay_r) = self.compute_delays(
            input_energy,
            wow_depth,
            flutter_depth,
            bog_depth,
            pinch_depth,
            azimuth_depth,
        );

        // Read interpolated samples
        let out_l = self.read_interpolated(&self.buffer_l, delay_l);
        let out_r = self.read_interpolated(&self.buffer_r, delay_r);

        // Advance circular buffer write position
        self.write_pos = (self.write_pos + 1) % MAX_DELAY_SAMPLES;

        (out_l, out_r)
    }

    /// 4-point Hermite cubic interpolation for silky smooth pitch changes
    #[inline(always)]
    fn read_interpolated(&self, buffer: &[f32], delay: f32) -> f32 {
        let read_pos = (self.write_pos as f32 + MAX_DELAY_SAMPLES as f32 - delay) % MAX_DELAY_SAMPLES as f32;
        let i0 = read_pos.floor() as usize;
        let frac = read_pos - i0 as f32;

        let im1 = (i0 + MAX_DELAY_SAMPLES - 1) % MAX_DELAY_SAMPLES;
        let i1 = (i0 + 1) % MAX_DELAY_SAMPLES;
        let i2 = (i0 + 2) % MAX_DELAY_SAMPLES;

        let y_m1 = buffer[im1];
        let y0 = buffer[i0];
        let y1 = buffer[i1];
        let y2 = buffer[i2];

        // 4-point, 3rd-order Hermite interpolation
        let c0 = y0;
        let c1 = 0.5 * (y1 - y_m1);
        let c2 = y_m1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - y_m1) + 1.5 * (y0 - y1);

        ((c3 * frac + c2) * frac + c1) * frac + c0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_motor_nominal_delay_impulse() {
        let sample_rate = 48000.0;
        let mut motor = MotorProcessor::new(sample_rate);
        assert_eq!(motor.nominal_delay_samples(), 720);

        let mut output_at = None;
        for i in 0..1000 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            let (out_l, out_r) = motor.process_stereo(input, input, 0.0, 0.0, 0.0, 0.0, 0.0);
            if out_l.abs() > 0.5 {
                assert!(output_at.is_none(), "Multiple impulses detected");
                output_at = Some(i);
                assert!((out_l - 1.0).abs() < 1e-4);
                assert!((out_r - 1.0).abs() < 1e-4);
            }
        }
        assert_eq!(output_at, Some(720));
    }

    #[test]
    fn test_wow_oscillator_phase_continuity() {
        let sample_rate = 48000.0;
        let mut motor = MotorProcessor::new(sample_rate);

        // Run through full wow cycle across wrap point
        let mut max_delta: f32 = 0.0;
        let mut prev_delay: Option<f32> = None;

        for _ in 0..70000 {
            let wow_inc = (2.0 * PI * motor.wow_rate) / motor.sample_rate;
            motor.wow_phase += wow_inc;
            if motor.wow_phase >= 2.0 * PI {
                motor.wow_phase -= 2.0 * PI;
            }
            let wow_inc2 = (2.0 * PI * motor.wow_rate * 2.3) / motor.sample_rate;
            motor.wow_phase2 += wow_inc2;
            if motor.wow_phase2 >= 2.0 * PI {
                motor.wow_phase2 -= 2.0 * PI;
            }
            let wow_mod = (motor.wow_phase.sin() + 0.3 * motor.wow_phase2.sin()) * 1.0 * 14.0;

            if let Some(prev) = prev_delay {
                let delta = (wow_mod - prev).abs();
                if delta > max_delta {
                    max_delta = delta;
                }
            }
            prev_delay = Some(wow_mod);
        }

        // Before the fix, max_delta was ~4.0 samples. With continuous phases, max delta is < 0.01 samples.
        assert!(
            max_delta < 0.01,
            "Wow modulation discontinuity detected! Max delta was {}",
            max_delta
        );
    }

    #[test]
    fn test_wow_flutter_and_sample_rates() {
        let sample_rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];
        let mut peak_pitch_devs = Vec::new();
        const K: usize = 32;

        for &sr in &sample_rates {
            let mut motor = MotorProcessor::new(sr);
            let mut max_delta: f32 = 0.0;
            let mut history = [0.0f32; K];
            let mut hist_idx = 0;

            // Run for 13.0 seconds to cover at least one full cycle of the beating wow LFO (0.08 Hz fundamental = 12.5 s)
            let total_samples = (13.0 * sr) as usize;
            for i in 0..total_samples {
                let (delay_l, _) = motor.compute_delays(0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
                if i >= K {
                    let prev = history[hist_idx];
                    let delta = (delay_l - prev).abs() / K as f32;
                    if delta > max_delta {
                        max_delta = delta;
                    }
                }
                history[hist_idx] = delay_l;
                hist_idx = (hist_idx + 1) % K;
            }
            peak_pitch_devs.push((sr, max_delta));
        }

        let baseline = peak_pitch_devs[0].1;
        for &(sr, dev) in &peak_pitch_devs {
            let relative_error = (dev - baseline).abs() / baseline;
            assert!(
                relative_error < 0.02,
                "Sample rate {} Hz peak pitch deviation ({}) differs from baseline 44.1 kHz ({}) by {:.2}%",
                sr,
                dev,
                baseline,
                relative_error * 100.0
            );
        }
    }

    #[test]
    fn test_azimuth_sample_rate_scaling() {
        let sample_rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];
        let target_us = (4.0 / REFERENCE_SAMPLE_RATE) * 1_000_000.0; // 90.70295 us

        for &sr in &sample_rates {
            let mut motor = MotorProcessor::new(sr);
            let mut max_az_samples: f32 = 0.0;

            // Run for 4.0 seconds (one full cycle of 0.25 Hz azimuth wander)
            let total_samples = (4.0 * sr) as usize;
            for _ in 0..total_samples {
                let (dl, dr) = motor.compute_delays(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
                let az_diff = (dl - dr).abs() / 2.0;
                if az_diff > max_az_samples {
                    max_az_samples = az_diff;
                }
            }

            let measured_us = (max_az_samples / sr) * 1_000_000.0;
            let rel_error = (measured_us - target_us).abs() / target_us;

            assert!(
                rel_error < 0.001,
                "Azimuth excursion at {} Hz ({:.2} us) differs from target 44.1 kHz ({:.2} us) by {:.4}%",
                sr,
                measured_us,
                target_us,
                rel_error * 100.0
            );

            if (sr - 44100.0).abs() < 1.0 {
                assert!(
                    (max_az_samples - 4.0).abs() < 1e-4,
                    "Reference 44.1 kHz azimuth excursion must be exactly 4.0 samples (was {})",
                    max_az_samples
                );
            }
        }
    }

    #[test]
    fn test_motor_bog_headroom_and_over_range() {
        let sample_rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];
        let test_levels = [
            0.0f32,                // Digital silence
            0.1259,                // -18 dBFS
            0.5012,                // -6 dBFS
            1.0,                   // 0 dBFS
            1.9953,                // +6 dBFS
            3.9811,                // +12 dBFS
            15.8489,               // +24 dBFS
            100.0,                 // Extreme over-range (+40 dBFS)
        ];

        let ceiling = (MAX_DELAY_SAMPLES - 5) as f32; // 4091.0

        for &sr in &sample_rates {
            for &amp in &test_levels {
                let mut motor = MotorProcessor::new(sr);
                let mut max_observed_delay = 0.0f32;

                // Run for 15,000 samples to let envelope and modulators reach peak
                for _ in 0..15000 {
                    let (dl, dr) = motor.compute_delays(amp, 1.0, 1.0, 1.0, 1.0, 1.0);
                    let d_max = dl.max(dr);
                    if d_max > max_observed_delay {
                        max_observed_delay = d_max;
                    }
                }

                assert!(
                    max_observed_delay < ceiling,
                    "Delay buffer ceiling overrun! sr={} Hz, amp={}, delay={:.2} >= ceiling={:.2}",
                    sr,
                    amp,
                    max_observed_delay,
                    ceiling
                );

                let headroom = ceiling - max_observed_delay;
                assert!(
                    headroom >= 890.0,
                    "Headroom fell below 890 samples: sr={} Hz, amp={}, delay={:.2}, headroom={:.2}",
                    sr,
                    amp,
                    max_observed_delay,
                    headroom
                );
            }

            // Test non-finite input robustness (NaN and Inf)
            let mut motor = MotorProcessor::new(sr);
            let (dl_nan, dr_nan) = motor.compute_delays(f32::NAN, 1.0, 1.0, 1.0, 1.0, 1.0);
            assert!(dl_nan.is_finite() && dr_nan.is_finite(), "NaN input must produce finite delays");
            let (dl_inf, dr_inf) = motor.compute_delays(f32::INFINITY, 1.0, 1.0, 1.0, 1.0, 1.0);
            assert!(dl_inf.is_finite() && dr_inf.is_finite(), "Inf input must produce finite delays");
            assert!(dl_inf < ceiling && dr_inf < ceiling, "Inf input must not overrun buffer");
        }
    }
}
