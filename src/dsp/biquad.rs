use std::f32::consts::PI;

/// A stable, clean Direct Form Transposed II biquad filter.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.s1;
        self.s1 = self.b1 * input - self.a1 * output + self.s2;
        self.s2 = self.b2 * input - self.a2 * output;

        // Flush subnormal numbers to prevent CPU spikes
        if output.abs() < 1e-15 {
            0.0
        } else {
            output
        }
    }

    /// 2-pole Lowpass Filter
    pub fn set_lowpass(&mut self, sample_rate: f32, cutoff_hz: f32, q: f32) {
        let cutoff = cutoff_hz.clamp(20.0, sample_rate * 0.49);
        let q = q.max(0.01);
        let omega = 2.0 * PI * cutoff / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q);

        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cos_omega) / 2.0) / a0;
        self.b1 = (1.0 - cos_omega) / a0;
        self.b2 = ((1.0 - cos_omega) / 2.0) / a0;
        self.a1 = (-2.0 * cos_omega) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// 2-pole Highpass Filter
    pub fn set_highpass(&mut self, sample_rate: f32, cutoff_hz: f32, q: f32) {
        let cutoff = cutoff_hz.clamp(10.0, sample_rate * 0.49);
        let q = q.max(0.01);
        let omega = 2.0 * PI * cutoff / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q);

        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 + cos_omega) / 2.0) / a0;
        self.b1 = -(1.0 + cos_omega) / a0;
        self.b2 = ((1.0 + cos_omega) / 2.0) / a0;
        self.a1 = (-2.0 * cos_omega) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// Peaking Bell Filter (Head Bump EQ)
    pub fn set_peaking(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
        let freq = freq_hz.clamp(20.0, sample_rate * 0.49);
        let q = q.max(0.01);
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * freq / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * q);

        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = (-2.0 * cos_omega) / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = (-2.0 * cos_omega) / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }

    /// High Shelf Filter
    pub fn set_high_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
        let freq = freq_hz.clamp(100.0, sample_rate * 0.49);
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * freq / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / 2.0 * ((a + 1.0 / a) * (1.0 / 0.707 - 1.0) + 2.0).max(0.0).sqrt();

        let a0 = (a + 1.0) - (a - 1.0) * cos_omega + 2.0 * a.sqrt() * alpha;
        self.b0 = (a * ((a + 1.0) + (a - 1.0) * cos_omega + 2.0 * a.sqrt() * alpha)) / a0;
        self.b1 = (-2.0 * a * ((a - 1.0) + (a + 1.0) * cos_omega)) / a0;
        self.b2 = (a * ((a + 1.0) + (a - 1.0) * cos_omega - 2.0 * a.sqrt() * alpha)) / a0;
        self.a1 = (2.0 * ((a - 1.0) - (a + 1.0) * cos_omega)) / a0;
        self.a2 = ((a + 1.0) - (a - 1.0) * cos_omega - 2.0 * a.sqrt() * alpha) / a0;
    }
}
