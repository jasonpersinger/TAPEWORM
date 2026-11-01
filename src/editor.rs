use crate::dsp::spring::SpringRouting;
use crate::dsp::tape::TapeType;
use crate::{HumFreq, TapewormParams};
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{
        self, pos2, vec2, Color32, Pos2, Rect,
        Sense, Stroke, Ui,
    },
    EguiState,
};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

#[path = "editor_pixel_font.rs"]
mod pixel_font;
use pixel_font::{wrap_text, PixelFont};

pub struct SharedUiState {
    pub reel_angle: AtomicU32,
    pub input_level: AtomicU32,
    pub bog_activity: AtomicU32,
    pub is_side_b: AtomicBool,
}

impl SharedUiState {
    pub fn new() -> Self {
        Self {
            reel_angle: AtomicU32::new(0),
            input_level: AtomicU32::new(0),
            bog_activity: AtomicU32::new(0),
            is_side_b: AtomicBool::new(false),
        }
    }
}

struct EditorResources {
    font: PixelFont,
    background: egui::TextureHandle,
}

pub fn create_editor(
    params: Arc<TapewormParams>,
    ui_state: Arc<SharedUiState>,
    editor_state: Arc<EguiState>,
) -> Option<Box<dyn Editor>> {
    // Decompress the complete pixel art background asset once at startup
    let bg_compressed = include_bytes!("../assets/tapeworm_layout.bin");
    let bg_raw = miniz_oxide::inflate::decompress_to_vec_zlib(bg_compressed)
        .expect("Failed to decompress TAPEWORM background asset");
    let bg_image = egui::ColorImage::from_rgb([1536, 1024], &bg_raw);

    create_egui_editor(
        editor_state.clone(),
        None::<EditorResources>,
        move |ctx, resources| {
            let font = PixelFont::new(ctx);
            let background = ctx.load_texture(
                "tapeworm_pixel_bg_full",
                bg_image.clone(),
                egui::TextureOptions::NEAREST,
            );
            *resources = Some(EditorResources { font, background });
        },
        move |egui_ctx, setter, resources| {
            let resources = resources.as_ref().expect("editor resources initialized with the editor");
            let font = &resources.font;
            let texture = &resources.background;
            let mut style = (*egui_ctx.style()).clone();
            style.visuals.dark_mode = true;
            style.visuals.panel_fill = Color32::from_rgb(11, 11, 13);
            style.visuals.window_fill = Color32::from_rgb(11, 11, 13);
            egui_ctx.set_style(style);
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::from_rgb(11, 11, 13)))
                .show(egui_ctx, |ui| {
                    let avail_w = ui.available_width().max(100.0);
                    let avail_h = ui.available_height().max(100.0);
                    let aspect = 1536.0 / 1024.0;

                    let (canvas_w, canvas_h) = if avail_w / avail_h > aspect {
                        (avail_h * aspect, avail_h)
                    } else {
                        (avail_w, avail_w / aspect)
                    };

                    let canvas_rect = Rect::from_center_size(
                        ui.available_rect_before_wrap().center(),
                        vec2(canvas_w, canvas_h),
                    );

                    let sx = canvas_w / 1536.0;
                    let sy = canvas_h / 1024.0;

                    let to_pos = |x: f32, y: f32| pos2(canvas_rect.left() + x * sx, canvas_rect.top() + y * sy);
                    let to_rect = |x1: f32, y1: f32, x2: f32, y2: f32| {
                        Rect::from_min_max(to_pos(x1, y1), to_pos(x2, y2))
                    };
                    let to_rad = |r: f32| r * sx;

                    // 1. RENDER BASE PIXEL-ART CASSETTE AND DECK CONSOLE
                    ui.painter().image(
                        texture.id(),
                        canvas_rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    // 2. ANIMATED CASSETTE REEL SPOKE SPINNERS
                    let reel_angle = (ui_state.reel_angle.load(Ordering::Relaxed) as f32) * 0.0015;
                    draw_spool_spokes(ui.painter(), to_pos(400.0, 220.0), to_rad(24.0), reel_angle, sx);
                    draw_spool_spokes(ui.painter(), to_pos(780.0, 220.0), to_rad(24.0), reel_angle * 1.05, sx);

                    let mut hovered_hint: Option<(&str, &str)> = None;

                    // 3. TOP RIGHT MASTER ROTARY KNOBS
                    let top_k_rad = to_rad(65.0);
                    let top_needle_len = to_rad(44.0);
                    let top_needle_thick = 10.0 * sx;

                    // DRY / WET
                    let dw_val = format!("{:.0}%", params.dry_wet.value() * 100.0);
                    let dw_hover = draw_deck_knob(
                        ui,
                        to_pos(1153.0, 256.5),
                        top_k_rad,
                        top_needle_len,
                        top_needle_thick,
                        sx,
                        &params.dry_wet,
                        setter,
                    );
                    if dw_hover {
                        hovered_hint = Some((
                            "DRY / WET",
                            "Mix between unprocessed dry signal and analog cassette path.",
                        ));
                    }

                    // OUTPUT TRIM
                    let out_db = util::gain_to_db(params.output_gain.value());
                    let out_val = format!("{:.1} dB", out_db);
                    let out_hover = draw_deck_knob(
                        ui,
                        to_pos(1383.0, 256.5),
                        top_k_rad,
                        top_needle_len,
                        top_needle_thick,
                        sx,
                        &params.output_gain,
                        setter,
                    );
                    if out_hover {
                        hovered_hint = Some((
                            "OUTPUT TRIM",
                            "Master output gain trim (-12dB to +12dB) with clean headroom.",
                        ));
                    }

                    draw_knob_value(ui, font, to_pos(1153.0, 360.0), &dw_val, 4.0 * sx);
                    draw_knob_value(ui, font, to_pos(1383.0, 360.0), &out_val, 4.0 * sx);

                    // 4. MIDDLE SELECTOR BUTTONS
                    let btn_y1 = 420.0;
                    let btn_y2 = 468.0;

                    // Tape Type: TYPE I (FERRIC)
                    let t1_rect = to_rect(205.0, btn_y1, 443.0, btn_y2);
                    let (t1_hover, t1_click) = draw_selector_button(ui, font, t1_rect, "TYPE I (FERRIC)", params.tape_type.value() == TapeType::Ferric, sx);
                    if t1_click {
                        setter.begin_set_parameter(&params.tape_type);
                        setter.set_parameter(&params.tape_type, TapeType::Ferric);
                        setter.end_set_parameter(&params.tape_type);
                    }
                    if t1_hover {
                        hovered_hint = Some((
                            "TYPE I / FERRIC",
                            "Warm ferric formulation with low-mid harmonic saturation and natural tape compression.",
                        ));
                    }

                    // Tape Type: TYPE II (CHROME)
                    let t2_rect = to_rect(457.0, btn_y1, 700.0, btn_y2);
                    let (t2_hover, t2_click) = draw_selector_button(ui, font, t2_rect, "TYPE II (CHROME)", params.tape_type.value() == TapeType::Chrome, sx);
                    if t2_click {
                        setter.begin_set_parameter(&params.tape_type);
                        setter.set_parameter(&params.tape_type, TapeType::Chrome);
                        setter.end_set_parameter(&params.tape_type);
                    }
                    if t2_hover {
                        hovered_hint = Some((
                            "TYPE II / CHROME",
                            "High-bias chrome formulation: punchy transients, extended highs, and cleaner headroom.",
                        ));
                    }

                    // Tape Type: MICRO (LO-FI)
                    let tm_rect = to_rect(713.0, btn_y1, 924.0, btn_y2);
                    let (tm_hover, tm_click) = draw_selector_button(ui, font, tm_rect, "MICRO (LO-FI)", params.tape_type.value() == TapeType::MicroCassette, sx);
                    if tm_click {
                        setter.begin_set_parameter(&params.tape_type);
                        setter.set_parameter(&params.tape_type, TapeType::MicroCassette);
                        setter.end_set_parameter(&params.tape_type);
                    }
                    if tm_hover {
                        hovered_hint = Some((
                            "MICRO / LO-FI",
                            "Murky microcassette dictation tape: narrow bandwidth, heavy grit, and crunchy mid-focus.",
                        ));
                    }

                    // Reverb Routing Toggle: PRE-TAPE / POST-TAPE
                    let rout_rect = to_rect(1088.0, btn_y1, 1241.0, btn_y2);
                    let is_pre = params.spring_routing.value() == SpringRouting::PreTape;
                    let (rout_hover, rout_click) = draw_selector_button(ui, font, rout_rect, if is_pre { "PRE-TAPE" } else { "POST-TAPE" }, true, sx);
                    if rout_click {
                        let next_routing = if is_pre {
                            SpringRouting::PostTape
                        } else {
                            SpringRouting::PreTape
                        };
                        setter.begin_set_parameter(&params.spring_routing);
                        setter.set_parameter(&params.spring_routing, next_routing);
                        setter.end_set_parameter(&params.spring_routing);
                    }
                    if rout_hover {
                        hovered_hint = Some((
                            "SPRING ROUTING",
                            if is_pre {
                                "PRE-TAPE: Spring reverb signal is driven INTO the cassette tape saturation and motor bog."
                            } else {
                                "POST-TAPE: Cassette saturation output feeds into the clean dungeon spring tank."
                            },
                        ));
                    }

                    // Mains Hum Toggle: 60Hz (US) / 50Hz (EU)
                    let hum_rect = to_rect(1371.0, btn_y1, 1503.0, btn_y2);
                    let is_60hz = params.hum_freq.value() == HumFreq::Hz60;
                    let (hum_hover, hum_click) = draw_selector_button(ui, font, hum_rect, if is_60hz { "60Hz (US)" } else { "50Hz (EU)" }, false, sx);
                    if hum_click {
                        let next_hum = if is_60hz { HumFreq::Hz50 } else { HumFreq::Hz60 };
                        setter.begin_set_parameter(&params.hum_freq);
                        setter.set_parameter(&params.hum_freq, next_hum);
                        setter.end_set_parameter(&params.hum_freq);
                    }
                    if hum_hover {
                        hovered_hint = Some((
                            "MAINS HUM FREQUENCY",
                            if is_60hz {
                                "60Hz: North American transformer electromagnetic induction fundamental & odd harmonics."
                            } else {
                                "50Hz: European / UK mains electromagnetic induction fundamental & odd harmonics."
                            },
                        ));
                    }

                    // 5. BOTTOM 16 MODULE KNOBS (4 PANELS x 4 KNOBS)
                    let k_rad = to_rad(45.0);
                    let k_needle_len = to_rad(28.0);
                    let k_needle_thick = 6.0 * sx;

                    // Row Y coordinates calibrated to exact disk centroids
                    let r1_y = 631.5;
                    let r2_y = 815.0;

                    let c1_x = 111.5;
                    let c2_x = 286.0;
                    let c3_x = 494.5;
                    let c4_x = 667.0;
                    let c5_x = 871.0;
                    let c6_x = 1046.0;
                    let c7_x = 1249.0;
                    let c8_x = 1427.0;

                    // --- PANEL 1: TAPE DRIVE & TONE ---
                    // DRIVE
                    let drv_val = format!("{:.1} dB", util::gain_to_db(params.drive.value()));
                    if draw_deck_knob(ui, to_pos(c1_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.drive, setter) {
                        hovered_hint = Some(("TAPE DRIVE", "Soft-knee cassette saturation, tape compression, and 3rd harmonic glue."));
                    }

                    // BIAS
                    let bias_val = format!("{:.0}", params.bias.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c2_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.bias, setter) {
                        hovered_hint = Some(("TAPE BIAS", "Magnetic operating point: underbias creates grit; overbias smooths saturation."));
                    }

                    // HEAD BUMP
                    let hb_val = format!("{:.1} dB", params.head_bump.value());
                    if draw_deck_knob(ui, to_pos(c1_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.head_bump, setter) {
                        hovered_hint = Some(("HEAD BUMP", "Playback head contour body resonance (80-120Hz) from tape-to-head contact."));
                    }

                    // TAPE AGE
                    let age_val = format!("{:.0}", params.age.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c2_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.age, setter) {
                        hovered_hint = Some(("TAPE AGE", "High-frequency attenuation and tape demagnetization from decades of aging."));
                    }

                    // --- PANEL 2: MOTOR WOBBLE (SAG) ---
                    // WOW
                    let wow_val = format!("{:.0}", params.wow.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c3_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.wow, setter) {
                        hovered_hint = Some(("WOW", "Slow periodic pitch sway (0.5-2Hz) caused by capstan motor speed variations."));
                    }

                    // FLUTTER
                    let flut_val = format!("{:.0}", params.flutter.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c4_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.flutter, setter) {
                        hovered_hint = Some(("FLUTTER", "Rapid scrape flutter and micro-jitter (6-15Hz) from tape friction across heads."));
                    }

                    // MOTOR BOG
                    let bog_val = format!("{:.0}", params.motor_bog.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c3_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.motor_bog, setter) {
                        hovered_hint = Some(("MOTOR BOG", "Dynamic pitch dip on loud bass transients & heavy chords, simulating dying batteries."));
                    }

                    // PINCH THUMP
                    let pt_val = format!("{:.0}", params.pinch_bump.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c4_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.pinch_bump, setter) {
                        hovered_hint = Some(("PINCH THUMP", "Cyclical 1.1Hz low-end thump from an indented or flat-spotted rubber pinch-roller."));
                    }

                    // --- PANEL 3: WEAR & DEGRADATION ---
                    // BOUNCES
                    let bnc_val = format!("{}", params.generations.value());
                    if draw_deck_knob(ui, to_pos(c5_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.generations, setter) {
                        hovered_hint = Some(("BOUNCES", "4-track cassette ping-pong dubs (1-4 passes), multiplying cumulative saturation."));
                    }

                    // DROPOUTS
                    let drop_val = format!("{:.0}", params.dropouts.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c6_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.dropouts, setter) {
                        hovered_hint = Some(("DROPOUTS", "Random tape crinkle and physical oxide flaking causing momentary channel dropouts."));
                    }

                    // TAPE HISS
                    let hiss_val = format!("{:.0}", params.hiss.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c5_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.hiss, setter) {
                        hovered_hint = Some(("TAPE HISS", "Authentic 1/f ferric magnetic hiss (auto-muted when transport or playback is idle)."));
                    }

                    // GROUND HUM
                    let hum_val = format!("{:.0}", params.hum.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c6_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.hum, setter) {
                        hovered_hint = Some(("GROUND HUM", "Electromagnetic transformer ground hum with odd power-line harmonics."));
                    }

                    // --- PANEL 4: DUB SPRING TANK ---
                    // SPRING TANK
                    let sp_val = format!("{:.0}", params.spring_amount.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c7_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.spring_amount, setter) {
                        hovered_hint = Some(("SPRING TANK", "Metallic vintage dual-spring reverb tank modeled on vintage portable cassette dub gear."));
                    }

                    // DECAY TIME
                    let dec_val = format!("{:.0}", params.spring_decay.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c8_x, r1_y), k_rad, k_needle_len, k_needle_thick, sx, &params.spring_decay, setter) {
                        hovered_hint = Some(("DECAY TIME", "Decay time and sustain length inside the metallic spring reverb tank."));
                    }

                    // DARK DAMP
                    let dmp_val = format!("{:.0}", params.spring_damp.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c7_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.spring_damp, setter) {
                        hovered_hint = Some(("DARK DAMP", "Internal high-frequency damping to keep the dungeon spring dark and cavernous."));
                    }

                    // AZIMUTH
                    let az_val = format!("{:.0}", params.azimuth.value() * 100.0);
                    if draw_deck_knob(ui, to_pos(c8_x, r2_y), k_rad, k_needle_len, k_needle_thick, sx, &params.azimuth, setter) {
                        hovered_hint = Some(("AZIMUTH SKEW", "Wandering stereo phase delay from an uncalibrated tape head, creating stereo drift."));
                    }

                    // Read the same live parameter values as the indicators on every frame.
                    for (x, y, value) in [
                        (c1_x, 706.0, &drv_val), (c2_x, 706.0, &bias_val),
                        (c3_x, 706.0, &wow_val), (c4_x, 706.0, &flut_val),
                        (c5_x, 706.0, &bnc_val), (c6_x, 706.0, &drop_val),
                        (c7_x, 706.0, &sp_val), (c8_x, 706.0, &dec_val),
                        (c1_x, 891.0, &hb_val), (c2_x, 891.0, &age_val),
                        (c3_x, 891.0, &bog_val), (c4_x, 891.0, &pt_val),
                        (c5_x, 891.0, &hiss_val), (c6_x, 891.0, &hum_val),
                        (c7_x, 891.0, &dmp_val), (c8_x, 891.0, &az_val),
                    ] {
                        draw_knob_value(ui, font, to_pos(x, y), value, 3.0 * sx);
                    }

                    // 6. BOTTOM STATUS / HINT FOOTER
                    let status_rect = to_rect(28.0, 944.0, 1508.0, 986.0);
                    draw_status_bar(ui, font, status_rect, hovered_hint, sx);
                });
        },
    )
}

/// A single geometry and bitmap alphabet for every selector state.
fn draw_selector_button(
    ui: &mut Ui,
    font: &PixelFont,
    rect: Rect,
    label: &str,
    selected: bool,
    scale: f32,
) -> (bool, bool) {
    let response = ui.allocate_rect(rect, Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter().with_clip_rect(rect);
    let border = if selected {
        Color32::from_rgb(255, 175, 60)
    } else if hovered {
        Color32::from_rgb(130, 130, 136)
    } else {
        Color32::from_rgb(75, 75, 79)
    };
    let face = if selected {
        Color32::from_rgb(243, 141, 40)
    } else {
        Color32::from_rgb(20, 20, 22)
    };
    let ink = if selected {
        Color32::from_rgb(20, 20, 22)
    } else {
        Color32::from_rgb(200, 200, 204)
    };
    paint_stepped_rect(&painter, rect, 4.0 * scale, border);
    paint_stepped_rect(&painter, rect.shrink(2.0 * scale), 2.0 * scale, Color32::from_rgb(11, 11, 13));
    paint_stepped_rect(&painter, rect.shrink(4.0 * scale), 2.0 * scale, face);

    let cell = vec2(2.25 * scale, 3.0 * scale);
    let size = vec2(PixelFont::width(label, cell.x), 7.0 * cell.y);
    font.draw_cells(&painter, label, rect.center() - size * 0.5, cell, ink);
    (hovered, response.clicked())
}

fn paint_stepped_rect(painter: &egui::Painter, rect: Rect, step: f32, color: Color32) {
    painter.rect_filled(
        Rect::from_min_max(rect.min + vec2(step, 0.0), rect.max - vec2(step, 0.0)),
        0,
        color,
    );
    painter.rect_filled(
        Rect::from_min_max(rect.min + vec2(0.0, step), rect.max - vec2(0.0, step)),
        0,
        color,
    );
}

/// One wheel tick advances one discrete value, or 2% for continuous knobs.
fn knob_scroll_value<P: Param>(param: &P, current: f32, scroll_y: f32) -> f32 {
    // NIH-plug reports intervals: BOUNCES has four values and three steps.
    // Thus 1 / intervals is 1 / (number_of_values - 1).
    let step = param
        .step_count()
        .map_or(0.02, |intervals| 1.0 / intervals.max(1) as f32);
    let normalized = (current + scroll_y.signum() * step).clamp(0.0, 1.0);
    param.preview_normalized(param.preview_plain(normalized))
}

/// Handles an interactive knob with drag controls, accumulator, and live rotating needle indicator
fn draw_deck_knob<P: Param>(
    ui: &mut Ui,
    center: Pos2,
    hit_radius: f32,
    needle_len: f32,
    needle_thick: f32,
    scale: f32,
    param: &P,
    setter: &ParamSetter,
) -> bool {
    let knob_rect = Rect::from_center_size(center, vec2(hit_radius * 2.0, hit_radius * 2.0));
    let response = ui.allocate_rect(knob_rect, Sense::click_and_drag());

    // Keep fractional motion locally: host updates can arrive a frame later,
    // and reading back a quantized IntParam would lose sub-step movement.
    let drag_id = response.id.with("drag_value");
    let mut drag_accum = ui.data(|d| {
        d.get_temp::<f32>(drag_id)
            .unwrap_or_else(|| param.unmodulated_normalized_value())
    });

    if response.drag_started() {
        drag_accum = param.unmodulated_normalized_value();
        setter.begin_set_parameter(param);
    }

    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let sensitivity = if fine { 0.001 } else { 0.004 };
        let delta = -response.drag_delta().y * sensitivity;
        drag_accum = (drag_accum + delta).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(drag_id, drag_accum));
        // Send the snapped value to the host, retaining the fractional drag
        // accumulator so BOUNCES advances steadily at the same sensitivity.
        setter.set_parameter(param, param.preview_plain(drag_accum));
    }

    if response.drag_stopped() {
        setter.end_set_parameter(param);
        ui.data_mut(|d| d.remove::<f32>(drag_id));
    }

    // Wheel/reset are independent gestures; never nest them inside a drag.
    if !response.dragged() {
        if response.double_clicked() {
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, param.default_normalized_value());
            setter.end_set_parameter(param);
        } else if response.hovered() {
            let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
            if scroll_y.abs() > 0.1 {
                let new_val =
                    knob_scroll_value(param, param.unmodulated_normalized_value(), scroll_y);
                setter.begin_set_parameter(param);
                setter.set_parameter_normalized(param, new_val);
                setter.end_set_parameter(param);
            }
        }
    }

    let is_hovered = response.hovered();
    if is_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Sweep 270 degrees from the lower left, through the top, to the lower right.
    let val_norm = param.modulated_normalized_value();
    let angle = -PI * 0.5 + (val_norm - 0.5) * (1.5 * PI);

    let painter = ui.painter();
    // The background already contains clean charcoal faces. Draw only the
    // live indicator so no antialiased cover can expose a baked needle.
    let p_end = pos2(
        center.x + needle_len * angle.cos(),
        center.y + needle_len * angle.sin(),
    );
    let needle_color = if is_hovered {
        Color32::from_rgb(255, 175, 60)
    } else {
        Color32::from_rgb(243, 141, 40)
    };
    painter.line_segment([center, p_end], Stroke::new(needle_thick, needle_color));

    // Central pivot pin
    let pin_rad = 3.0 * scale;
    painter.circle_filled(center, pin_rad, Color32::from_rgb(20, 20, 22));

    is_hovered || response.dragged()
}

/// Draws rotating cassette spool spokes inside the cassette window
fn draw_spool_spokes(painter: &egui::Painter, center: Pos2, radius: f32, angle: f32, sx: f32) {
    let tooth_count = 3;
    let tooth_w = 4.5 * sx;
    for i in 0..tooth_count {
        let th = angle + (i as f32) * (2.0 * PI / tooth_count as f32);
        let p1 = pos2(center.x + (radius * 0.45) * th.cos(), center.y + (radius * 0.45) * th.sin());
        let p2 = pos2(center.x + radius * th.cos(), center.y + radius * th.sin());
        painter.line_segment([p1, p2], Stroke::new(tooth_w, Color32::from_rgb(18, 18, 20)));
    }
}

/// Center every readout on its knob, using the same bitmap alphabet as the footer.
fn draw_knob_value(ui: &Ui, font: &PixelFont, center: Pos2, text: &str, pixel: f32) {
    let origin = center - vec2(PixelFont::width(text, pixel), 7.0 * pixel) * 0.5;
    font.draw(ui.painter(), text, origin, pixel, Color32::from_rgb(200, 200, 204));
}

/// Stable columns and wrapped text inside the artwork's existing footer border.
fn draw_status_bar(
    ui: &Ui,
    font: &PixelFont,
    rect: Rect,
    hovered: Option<(&str, &str)>,
    sx: f32,
) {
    let (title, hint) = hovered.unwrap_or((
        "TAPEWORM",
        "Drag to adjust. Shift for fine control. Scroll to step. Double-click to reset.",
    ));
    let painter = ui.painter().with_clip_rect(rect);
    let title_origin = pos2(rect.left() + 12.0 * sx, rect.center().y - 10.5 * sx);
    font.draw(&painter, title, title_origin, 3.0 * sx, Color32::from_rgb(243, 141, 40));

    let divider_x = rect.left() + 366.0 * sx;
    painter.rect_filled(
        Rect::from_min_max(pos2(divider_x, rect.top() + 4.0 * sx), pos2(divider_x + 2.0 * sx, rect.bottom() - 4.0 * sx)),
        0,
        Color32::from_rgb(75, 75, 79),
    );
    let text_x = divider_x + 24.0 * sx;
    let pixel = 2.25 * sx;
    let columns = ((rect.right() - 12.0 * sx - text_x) / (6.0 * pixel)) as usize;
    let lines = wrap_text(hint, columns);
    let line_height = 9.0 * pixel;
    let block_height = (lines.len().saturating_sub(1)) as f32 * line_height + 7.0 * pixel;
    let top = rect.center().y - block_height * 0.5;
    for (i, line) in lines.iter().enumerate() {
        font.draw(&painter, line, pos2(text_x, top + i as f32 * line_height), pixel, Color32::from_rgb(200, 200, 204));
    }
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;
