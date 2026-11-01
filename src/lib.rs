use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub mod dsp;
pub mod editor;

use dsp::motor::MotorProcessor;
use dsp::spring::{DungeonSpring, SpringRouting};
use dsp::tape::{TapeProcessor, TapeType};
use dsp::wear::WearProcessor;
use editor::{create_editor, SharedUiState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum HumFreq {
    #[name = "50 Hz (Europe / UK)"]
    Hz50,
    #[name = "60 Hz (North America)"]
    Hz60,
}

pub struct Tapeworm {
    params: Arc<TapewormParams>,
    sample_rate: f32,

    // DSP Engine Modules
    tape: TapeProcessor,
    motor: MotorProcessor,
    wear: WearProcessor,
    spring: DungeonSpring,

    // Latency compensation for dry path matching motor latency
    dry_buffer_l: Vec<f32>,
    dry_buffer_r: Vec<f32>,
    dry_pos: usize,
    dry_latency: usize,

    // Shared state with GUI
    ui_state: Arc<SharedUiState>,
    reel_counter: f32,
}

#[derive(Params)]
pub struct TapewormParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    // --- Tape Engine ---
    #[id = "tape_type"]
    pub tape_type: EnumParam<TapeType>,

    #[id = "drive"]
    pub drive: FloatParam,

    #[id = "bias"]
    pub bias: FloatParam,

    #[id = "head_bump"]
    pub head_bump: FloatParam,

    #[id = "age"]
    pub age: FloatParam,

    // --- Motor Mechanics & Bog ---
    #[id = "wow"]
    pub wow: FloatParam,

    #[id = "flutter"]
    pub flutter: FloatParam,

    #[id = "motor_bog"]
    pub motor_bog: FloatParam,

    #[id = "pinch_bump"]
    pub pinch_bump: FloatParam,

    #[id = "azimuth"]
    pub azimuth: FloatParam,

    // --- Wear & Degradation ---
    #[id = "generations"]
    pub generations: IntParam,

    #[id = "dropouts"]
    pub dropouts: FloatParam,

    #[id = "hiss"]
    pub hiss: FloatParam,

    #[id = "hum"]
    pub hum: FloatParam,

    #[id = "hum_freq"]
    pub hum_freq: EnumParam<HumFreq>,

    // --- Dungeon Spring Reverb ---
    #[id = "spring_amount"]
    pub spring_amount: FloatParam,

    #[id = "spring_decay"]
    pub spring_decay: FloatParam,

    #[id = "spring_damp"]
    pub spring_damp: FloatParam,

    #[id = "spring_routing"]
    pub spring_routing: EnumParam<SpringRouting>,

    // --- Master ---
    #[id = "dry_wet"]
    pub dry_wet: FloatParam,

    #[id = "output_gain"]
    pub output_gain: FloatParam,
}

impl Default for Tapeworm {
    fn default() -> Self {
        let sample_rate = 44100.0;
        let motor = MotorProcessor::new(sample_rate);
        let dry_latency = motor.nominal_delay_samples();
        Self {
            params: Arc::new(TapewormParams::default()),
            sample_rate,
            tape: TapeProcessor::new(sample_rate),
            motor,
            wear: WearProcessor::new(sample_rate),
            spring: DungeonSpring::new(sample_rate),
            dry_buffer_l: vec![0.0; dry_latency.max(1)],
            dry_buffer_r: vec![0.0; dry_latency.max(1)],
            dry_pos: 0,
            dry_latency,
            ui_state: Arc::new(SharedUiState::new()),
            reel_counter: 0.0,
        }
    }
}

impl Default for TapewormParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(1152, 768),

            // Tape Engine
            tape_type: EnumParam::new("Tape Type", TapeType::Ferric),
            drive: FloatParam::new(
                "Drive",
                util::db_to_gain(6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(0.0),
                    max: util::db_to_gain(24.0),
                    factor: FloatRange::gain_skew_factor(0.0, 24.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(30.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            bias: FloatParam::new(
                "Bias",
                0.15,
                FloatRange::Linear { min: -1.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            head_bump: FloatParam::new(
                "Head Bump",
                3.5,
                FloatRange::Linear { min: 0.0, max: 9.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            age: FloatParam::new(
                "Tape Age",
                0.25,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            // Motor Dynamics
            wow: FloatParam::new(
                "Wow Depth",
                0.35,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            flutter: FloatParam::new(
                "Flutter Depth",
                0.20,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            motor_bog: FloatParam::new(
                "Motor Bog",
                0.45,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            pinch_bump: FloatParam::new(
                "Pinch Bump",
                0.15,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            azimuth: FloatParam::new(
                "Azimuth Skew",
                0.25,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            // Wear & Degradation
            generations: IntParam::new(
                "Generations",
                2,
                IntRange::Linear { min: 1, max: 4 },
            ),

            dropouts: FloatParam::new(
                "Dropouts",
                0.20,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            hiss: FloatParam::new(
                "Tape Hiss",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            hum: FloatParam::new(
                "Ground Hum",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            hum_freq: EnumParam::new("Hum Frequency", HumFreq::Hz60),

            // Dungeon Spring
            spring_amount: FloatParam::new(
                "Spring Level",
                0.25,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            spring_decay: FloatParam::new(
                "Spring Decay",
                0.50,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            spring_damp: FloatParam::new(
                "Spring Damping",
                0.60,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            spring_routing: EnumParam::new("Spring Routing", SpringRouting::PreTape),

            // Master
            dry_wet: FloatParam::new(
                "Dry / Wet",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            output_gain: FloatParam::new(
                "Output Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-24.0),
                    max: util::db_to_gain(12.0),
                    factor: FloatRange::gain_skew_factor(-24.0, 12.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(30.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Plugin for Tapeworm {
    const NAME: &'static str = "TAPEWORM";
    const VENDOR: &'static str = "MESSYQUEST";
    const URL: &'static str = "https://tapeworm.messy.quest";
    const EMAIL: &'static str = "info@messy.quest";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        create_editor(
            self.params.clone(),
            self.ui_state.clone(),
            self.params.editor_state.clone(),
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.tape.reset(self.sample_rate);
        self.motor.reset(self.sample_rate);
        self.wear.reset(self.sample_rate);
        self.spring.set_sample_rate(self.sample_rate);

        let latency = self.motor.nominal_delay_samples();
        self.dry_latency = latency;
        self.dry_buffer_l = vec![0.0; latency.max(1)];
        self.dry_buffer_r = vec![0.0; latency.max(1)];
        self.dry_pos = 0;

        context.set_latency_samples(latency as u32);
        true
    }

    fn reset(&mut self) {
        self.tape.reset(self.sample_rate);
        self.motor.reset(self.sample_rate);
        self.wear.reset(self.sample_rate);
        self.spring.reset(self.sample_rate);

        self.dry_buffer_l.fill(0.0);
        self.dry_buffer_r.fill(0.0);
        self.dry_pos = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Read active parameters
        let tape_type = self.params.tape_type.value();
        let generation = self.params.generations.value() as u8;
        let spring_routing = self.params.spring_routing.value();
        let hum_freq_hz = match self.params.hum_freq.value() {
            HumFreq::Hz50 => 50.0,
            HumFreq::Hz60 => 60.0,
        };

        self.wear.update_generation_filters(generation);

        let mut peak_in: f32 = 0.0;
        let mut sample_idx: usize = 0;

        for mut channel_samples in buffer.iter_samples() {
            let in_l = *channel_samples.get_mut(0).unwrap_or(&mut 0.0);
            let in_r = if channel_samples.len() > 1 {
                *channel_samples.get_mut(1).unwrap_or(&mut 0.0)
            } else {
                in_l
            };

            let samp_peak = in_l.abs().max(in_r.abs());
            if samp_peak > peak_in {
                peak_in = samp_peak;
            }

            // Smoothed parameters for sample-accurate audio
            let head_bump = self.params.head_bump.smoothed.next();
            let age = self.params.age.smoothed.next();

            // Update tape filters at a 16-sample control rate cadence (~0.33 ms at 48 kHz)
            if sample_idx % 16 == 0 {
                self.tape.update_filters_if_changed(tape_type, head_bump, age);
            }
            sample_idx += 1;

            let drive = self.params.drive.smoothed.next();
            let bias = self.params.bias.smoothed.next();
            let wow = self.params.wow.smoothed.next();
            let flutter = self.params.flutter.smoothed.next();
            let bog = self.params.motor_bog.smoothed.next();
            let pinch = self.params.pinch_bump.smoothed.next();
            let azimuth = self.params.azimuth.smoothed.next();
            let dropouts = self.params.dropouts.smoothed.next();
            let hiss = self.params.hiss.smoothed.next();
            let hum = self.params.hum.smoothed.next();
            let spring_amt = self.params.spring_amount.smoothed.next();
            let spring_dec = self.params.spring_decay.smoothed.next();
            let spring_dmp = self.params.spring_damp.smoothed.next();
            let dry_wet = self.params.dry_wet.smoothed.next();
            let out_gain = self.params.output_gain.smoothed.next();

            // Dry delay line compensation for motor latency
            let dry_l = if self.dry_latency > 0 {
                let d = self.dry_buffer_l[self.dry_pos];
                self.dry_buffer_l[self.dry_pos] = in_l;
                d
            } else {
                in_l
            };
            let dry_r = if self.dry_latency > 0 {
                let d = self.dry_buffer_r[self.dry_pos];
                self.dry_buffer_r[self.dry_pos] = in_r;
                self.dry_pos = (self.dry_pos + 1) % self.dry_latency;
                d
            } else {
                in_r
            };

            let mut l = in_l;
            let mut r = in_r;

            // 1. Pre-Tape Spring Reverb (haunting dungeon space chewed up by tape)
            if spring_routing == SpringRouting::PreTape {
                let (sl, sr) = self.spring.process_stereo(l, r, spring_amt, spring_dec, spring_dmp);
                l = sl;
                r = sr;
            }

            // 2. Tape Head Drive & Saturation
            let (tl, tr) = self.tape.process_stereo(l, r, drive, bias);
            l = tl;
            r = tr;

            // 3. Motor Mechanics: Wow, Flutter, Pitch Bog & Azimuth
            let (ml, mr) = self.motor.process_stereo(l, r, wow, flutter, bog, pinch, azimuth);
            l = ml;
            r = mr;

            // 4. Wear & Degradation: Generations, Dropouts, Hiss & Hum
            let (wl, wr) = self.wear.process_stereo(l, r, generation, dropouts, hiss, hum, hum_freq_hz);
            l = wl;
            r = wr;

            // 5. Post-Tape Spring Reverb
            if spring_routing == SpringRouting::PostTape {
                let (sl, sr) = self.spring.process_stereo(l, r, spring_amt, spring_dec, spring_dmp);
                l = sl;
                r = sr;
            }

            // 6. Dry / Wet Blend & Master Output Gain
            let final_l = (dry_l * (1.0 - dry_wet) + l * dry_wet) * out_gain;
            let final_r = (dry_r * (1.0 - dry_wet) + r * dry_wet) * out_gain;

            // Write back to buffer
            if let Some(sample) = channel_samples.get_mut(0) {
                *sample = final_l;
            }
            if channel_samples.len() > 1 {
                if let Some(sample) = channel_samples.get_mut(1) {
                    *sample = final_r;
                }
            }
        }

        // Advance UI reel animation counter proportional to signal presence
        let speed = if peak_in > 0.001 { 1.0 } else { 0.2 };
        self.reel_counter += speed * 0.08;
        self.ui_state.reel_angle.store(
            (self.reel_counter * 100.0) as u32,
            Ordering::Relaxed,
        );

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Tapeworm {
    const CLAP_ID: &'static str = "com.messyquest.tapeworm";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Vintage Lo-Fi Cassette & Dungeon Synth Tape Processor");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Distortion,
        ClapFeature::Filter,
        ClapFeature::Reverb,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for Tapeworm {
    const VST3_CLASS_ID: [u8; 16] = *b"TapewormDungeon1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Distortion,
        Vst3SubCategory::Reverb,
    ];
}

nih_export_clap!(Tapeworm);
nih_export_vst3!(Tapeworm);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_delay_alignment() {
        let mut tapeworm = Tapeworm::default();
        let sample_rate = 48000.0;
        tapeworm.sample_rate = sample_rate;
        tapeworm.motor.reset(sample_rate);
        let latency = tapeworm.motor.nominal_delay_samples();
        assert_eq!(latency, 720);
        tapeworm.dry_latency = latency;
        tapeworm.dry_buffer_l = vec![0.0; latency];
        tapeworm.dry_buffer_r = vec![0.0; latency];
        tapeworm.dry_pos = 0;

        let mut dry_out_at = None;
        for i in 0..1000 {
            let in_l = if i == 0 { 1.0 } else { 0.0 };
            let dry_l = if tapeworm.dry_latency > 0 {
                let d = tapeworm.dry_buffer_l[tapeworm.dry_pos];
                tapeworm.dry_buffer_l[tapeworm.dry_pos] = in_l;
                tapeworm.dry_pos = (tapeworm.dry_pos + 1) % tapeworm.dry_latency;
                d
            } else {
                in_l
            };
            if dry_l.abs() > 0.5 {
                dry_out_at = Some(i);
            }
        }
        assert_eq!(dry_out_at, Some(720));
    }

    #[test]
    fn test_percentage_parameter_parsing() {
        let params = TapewormParams::default();

        // 35 entered into wow should parse as 0.35, not clamp to 1.0
        let norm_35 = params.wow.string_to_normalized_value("35").expect("parses 35");
        let plain_35 = params.wow.preview_plain(norm_35);
        assert!((plain_35 - 0.35).abs() < 1e-4);

        let norm_35_pct = params.wow.string_to_normalized_value("35%").expect("parses 35%");
        let plain_35_pct = params.wow.preview_plain(norm_35_pct);
        assert!((plain_35_pct - 0.35).abs() < 1e-4);

        // Bias supports negative percentages (-1.0 to 1.0)
        let norm_bias = params.bias.string_to_normalized_value("-50%").expect("parses -50%");
        let plain_bias = params.bias.preview_plain(norm_bias);
        assert!((plain_bias - (-0.5)).abs() < 1e-4);

        // Other percentage params
        let norm_age = params.age.string_to_normalized_value("75").expect("parses 75");
        assert!((params.age.preview_plain(norm_age) - 0.75).abs() < 1e-4);

        let norm_flutter = params.flutter.string_to_normalized_value("20%").expect("parses 20%");
        assert!((params.flutter.preview_plain(norm_flutter) - 0.20).abs() < 1e-4);

        let norm_spring = params.spring_amount.string_to_normalized_value("50").expect("parses 50");
        assert!((params.spring_amount.preview_plain(norm_spring) - 0.50).abs() < 1e-4);

        let norm_dw = params.dry_wet.string_to_normalized_value("80%").expect("parses 80%");
        assert!((params.dry_wet.preview_plain(norm_dw) - 0.80).abs() < 1e-4);
    }

    #[test]
    fn test_public_parameter_inventory() {
        let params = TapewormParams::default();
        let param_map = params.param_map();

        // Exactly 21 host-exposed audio DSP parameters
        assert_eq!(
            param_map.len(),
            21,
            "Expected 21 public parameters, found {}",
            param_map.len()
        );

        let expected_params = [
            ("tape_type", "Tape Type"),
            ("drive", "Drive"),
            ("bias", "Bias"),
            ("head_bump", "Head Bump"),
            ("age", "Tape Age"),
            ("wow", "Wow Depth"),
            ("flutter", "Flutter Depth"),
            ("motor_bog", "Motor Bog"),
            ("pinch_bump", "Pinch Bump"),
            ("azimuth", "Azimuth Skew"),
            ("generations", "Generations"),
            ("dropouts", "Dropouts"),
            ("hiss", "Tape Hiss"),
            ("hum", "Ground Hum"),
            ("hum_freq", "Hum Frequency"),
            ("spring_amount", "Spring Level"),
            ("spring_decay", "Spring Decay"),
            ("spring_damp", "Spring Damping"),
            ("spring_routing", "Spring Routing"),
            ("dry_wet", "Dry / Wet"),
            ("output_gain", "Output Gain"),
        ];

        for (expected_id, expected_name) in expected_params {
            let found = param_map.iter().find(|(id, _, _)| id == expected_id);
            assert!(
                found.is_some(),
                "Mandatory parameter id '{}' missing from TapewormParams!",
                expected_id
            );
            let (_, param_ptr, _) = found.unwrap();
            let actual_name = unsafe { param_ptr.name() };
            assert_eq!(
                actual_name, expected_name,
                "Parameter name mismatch for '{}': expected '{}', got '{}'",
                expected_id, expected_name, actual_name
            );
        }
    }

    #[test]
    fn test_latency_reporting_across_sample_rates() {
        let sample_rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];
        for &sr in &sample_rates {
            let latency = MotorProcessor::calc_nominal_delay_samples(sr);
            let expected = (0.015 * sr).round() as usize;
            assert_eq!(
                latency, expected,
                "Latency reporting mismatch at {} Hz: expected {}, got {}",
                sr, expected, latency
            );
        }
    }

    #[test]
    fn test_full_dsp_pipeline_robustness() {
        let sample_rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];

        for &sr in &sample_rates {
            let mut tape = TapeProcessor::new(sr);
            let mut motor = MotorProcessor::new(sr);
            let mut wear = WearProcessor::new(sr);
            let mut spring = DungeonSpring::new(sr);

            let stimuli: [(&str, Box<dyn Fn(usize) -> f32>); 6] = [
                ("silence", Box::new(|_| 0.0)),
                ("very_quiet", Box::new(|_| 1e-5)),
                ("nominal_0dbfs", Box::new(|i| (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / sr).sin())),
                ("overload_24dbfs", Box::new(|i| 15.85 * (2.0 * std::f32::consts::PI * 100.0 * (i as f32) / sr).sin())),
                ("unit_impulse", Box::new(|i| if i == 0 { 1.0 } else { 0.0 })),
                ("noise", Box::new(|i| (((i * 1103515245 + 12345) & 0x7FFFFFFF) as f32 / 1073741824.0) - 1.0)),
            ];

            for (name, stim) in stimuli {
                tape.reset(sr);
                motor.reset(sr);
                wear.reset(sr);
                spring.reset(sr);

                for i in 0..1024 {
                    let in_val = stim(i);
                    let (sl, sr) = spring.process_stereo(in_val, in_val, 0.25, 0.50, 0.60);
                    let (tl, tr) = tape.process_stereo(sl, sr, 1.995, 0.15); // +6 dB drive
                    let (ml, mr) = motor.process_stereo(tl, tr, 0.35, 0.20, 0.45, 0.15, 0.25);
                    let (wl, wr) = wear.process_stereo(ml, mr, 2, 0.20, 0.05, 0.05, 60.0);

                    assert!(
                        wl.is_finite() && wr.is_finite(),
                        "Non-finite sample in stimulus '{}' at sr={} Hz, sample {}: l={}, r={}",
                        name, sr, i, wl, wr
                    );
                }
            }
        }
    }
}
