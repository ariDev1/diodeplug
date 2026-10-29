use diode_plug_lab::analysis;
use diode_plug_lab::export;
use diode_plug_lab::measurement;
use diode_plug_lab::observed::{Location, ObservedAc};
use diode_plug_lab::presets;
use diode_plug_lab::solver::{
    AcReturn, Circuit, LoadModel, PulseGate, ResultPoint, RingdownParams, RingdownResult,
    open_circuit_probe, ringdown,
};
use diode_plug_lab::sweep::{self, Config, Sweep};
use diode_plug_lab::units::{self, Unit};
use eframe::egui::{self, Color32, FontId, RichText, TextStyle};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_plot::{HoverPosition, Legend, Line, LineStyle, Plot, Points, VLine};
use latex_rust::{MathFont, SvgOptions, latex_to_svg};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TEAL: Color32 = Color32::from_rgb(86, 187, 180);
const BLUE: Color32 = Color32::from_rgb(117, 161, 224);
const ORANGE: Color32 = Color32::from_rgb(229, 174, 110);
const GOLD: Color32 = Color32::from_rgb(246, 211, 124);

const DOCUMENTS: [(&str, &str); 2] = [
    ("Circuit theory", include_str!("../diode-plug_explained.md")),
    ("Model coverage", include_str!("../MODEL_COVERAGE.md")),
];
const DIODE_FIGURE_MARKER: &str = "<!-- bundled-diode-plug-figure -->";
// A renderer-compatible copy of the user-supplied draw.io diagram (see scripts/prepare-doc-figure.py).
const DIODE_FIGURE: &[u8] = include_bytes!("../diode-plug_offline.svg");

struct DocumentSection {
    title: String,
    markdown: String,
}

static DOCUMENT_SECTIONS: LazyLock<[Vec<DocumentSection>; 2]> =
    LazyLock::new(|| DOCUMENTS.map(|(_, source)| parse_document(source)));
static MATH_FONT: LazyLock<Option<MathFont>> = LazyLock::new(|| MathFont::stix_two_math().ok());
static MATH_IMAGES: LazyLock<Mutex<HashMap<(String, bool), Arc<[u8]>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn parse_document(source: &str) -> Vec<DocumentSection> {
    // The default offline UI font has no U+2011 (non-breaking hyphen). Keep the
    // authored Markdown intact and use a legible hyphen in its displayed copy.
    let readable_source = source.replace('‑', "-");
    let mut sections = Vec::new();
    let mut title = String::new();
    let mut markdown = String::new();
    let mut front_matter = source.starts_with("---\n");
    let mut in_equation = false;
    for (index, line) in readable_source.lines().enumerate() {
        if front_matter {
            if index > 0 && line == "---" {
                front_matter = false;
            }
            continue;
        }
        if let Some(heading) = line
            .strip_prefix("## ")
            .or_else(|| line.strip_prefix("### "))
            .or_else(|| line.strip_prefix("# "))
        {
            if !title.is_empty() {
                sections.push(DocumentSection {
                    title,
                    markdown: normalize_inline_math(&markdown),
                });
                markdown = String::new();
            }
            title = heading.to_owned();
        }
        if line.trim() == r"\(" {
            in_equation = true;
            markdown.push('\n');
            markdown.push_str(&line[..line.len() - line.trim_start().len()]);
            markdown.push_str("$$\n");
        } else if line.trim() == r"\)" && in_equation {
            in_equation = false;
            markdown.push_str(&line[..line.len() - line.trim_start().len()]);
            markdown.push_str("$$\n\n");
        } else if line.trim_start().starts_with("![") {
            if line.contains("(diode-plug_simple.svg)") {
                markdown.push_str(DIODE_FIGURE_MARKER);
                markdown.push('\n');
                continue;
            }
            let caption = line
                .split_once(']')
                .map_or("Figure", |(caption, _)| caption.trim_start_matches("!["));
            markdown.push_str(&format!(
                "> **{caption}** — Figure asset is not included with this project.\n"
            ));
        } else {
            markdown.push_str(&line.replace(
                "[model coverage and sign-convention notes](MODEL_COVERAGE.md)",
                "**model coverage and sign-convention notes** (open Model coverage above)",
            ));
            markdown.push('\n');
        }
    }
    if !title.is_empty() {
        sections.push(DocumentSection {
            title,
            markdown: normalize_inline_math(&markdown),
        });
    }
    sections
}

/// Convert source `\(…\)` notation without leaving whitespace immediately
/// inside `$…$` (CommonMark deliberately does not recognize `$ x $`).
fn normalize_inline_math(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(open) = rest.find(r"\(") {
        let Some(close) = rest[open + 2..].find(r"\)") else {
            break;
        };
        output.push_str(&rest[..open]);
        let tex = rest[open + 2..open + 2 + close].trim();
        if tex.contains('\n') {
            output.push_str("\n$$\n");
            output.push_str(tex);
            output.push_str("\n$$\n");
        } else {
            output.push('$');
            output.push_str(tex);
            output.push('$');
        }
        rest = &rest[open + 2 + close + 2..];
    }
    output.push_str(rest);
    output
}

fn equation_svg(tex: &str, display: bool) -> Result<Arc<[u8]>, String> {
    let key = (tex.to_owned(), display);
    if let Some(cached) = MATH_IMAGES.lock().map_err(|e| e.to_string())?.get(&key) {
        return Ok(Arc::clone(cached));
    }
    let font = MATH_FONT
        .as_ref()
        .ok_or("Bundled math font could not be loaded")?;
    let mut options = SvgOptions::new();
    options.font_size_pt = latex_rust::Dim::from_i64(if display { 18 } else { 15 });
    options.color = latex_rust::Color::rgb(229, 237, 242);
    options.display = display;
    let bytes: Arc<[u8]> = latex_to_svg(tex, font, &options)
        .map_err(|error| format!("Equation cannot be typeset: {error}"))?
        .into_bytes()
        .into();
    MATH_IMAGES
        .lock()
        .map_err(|e| e.to_string())?
        .insert(key, Arc::clone(&bytes));
    Ok(bytes)
}

fn render_equation(ui: &mut egui::Ui, tex: &str, inline: bool) {
    let _ = render_equation_image(ui, tex, inline);
}

fn render_equation_image(ui: &mut egui::Ui, tex: &str, inline: bool) -> egui::Response {
    match equation_svg(tex, !inline) {
        Ok(bytes) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (tex, inline).hash(&mut hasher);
            let uri = format!("bytes://equation-{:x}.svg", hasher.finish());
            ui.add(
                egui::Image::from_bytes(uri, egui::load::Bytes::Shared(bytes))
                    .fit_to_original_size(1.0)
                    .max_width(ui.available_width().max(if inline { 180.0 } else { 440.0 })),
            )
            .on_hover_text(tex)
        }
        Err(error) => ui
            .label(RichText::new(tex).monospace().color(ORANGE))
            .on_hover_text(error),
    }
}

fn default_config() -> Config {
    Config {
        circuit: Circuit {
            voltage_rms: 24.0,
            frequency: 10_000.0,
            resistance: 100.0,
            inductance: 0.0034,
            load_capacitance: 0.001,
            source_resistance: 0.9,
            diode_drop: 0.7,
            diode_resistance: 1.0,
            load_model: LoadModel::Linear,
            ac_return: None,
            source_inductance: 0.0,
            capacitor_ratio: 1.0,
            capacitor_esr_ohm: 0.0,
            capacitor_leakage_parallel_ohm: None,
            diode_capacitance_f: 0.0,
            diode_recovery_charge_nc: 0.0,
            pulse_gate: None,
        },
        preset_name: None,
        capacitance_min_uf: 0.1,
        capacitance_max_uf: 10.0,
        points: 50,
        reference_ohms: Some(10.0),
        observed_ac: None,
        fit_to_observed_power: false,
        operator: String::new(),
        run_id: String::new(),
        workers: 0,
    }
}

fn parse_batch(args: &[String]) -> Result<(), String> {
    if args == ["--version"] {
        println!("diode-plug-lab {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args == ["--list-presets"] {
        for (i, p) in presets::all().iter().enumerate() {
            println!("{:2}. {} — {}", i + 1, p.name, p.note);
        }
        return Ok(());
    }
    if (args.len() == 4 || args.len() == 5) && args[0] == "--measure" && args[2] == "--frequency" {
        let inverted = args.len() == 5 && args[4] == "--invert-current";
        if args.len() == 5 && !inverted {
            return Err("Unknown measurement flag (expected --invert-current)".into());
        }
        let frequency = units::parse(&args[3], Unit::Hertz, 1.0)?;
        let measured = measurement::import_csv(Path::new(&args[1]), frequency, inverted)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&measured).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.len() != 4 || args[2] != "--output" {
        return Err("Usage: diode-plug-lab [--version | --list-presets | --batch config.json --output NEW_DIRECTORY | --preset NUMBER --output NEW_DIRECTORY | --measure wave.csv --frequency 50Hz [--invert-current]]".into());
    }
    let config: Config = match args[0].as_str() {
        "--batch" => serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid config: {e}"))?,
        "--preset" => {
            let id = args[1]
                .parse::<usize>()
                .map_err(|_| "Preset must be a number from --list-presets")?;
            presets::all()
                .get(id.checked_sub(1).ok_or("Preset numbers start at 1")?)
                .ok_or("Unknown preset number; use --list-presets")?
                .config
                .clone()
        }
        _ => return Err("Use --batch, --preset, or --list-presets".into()),
    };
    config.validate()?;
    let start = std::time::Instant::now();
    let sweep = sweep::run(config, |done, total| {
        eprintln!("{done}/{total} points completed")
    })?;
    let compute_time = start.elapsed();
    export::write(&sweep, Path::new(&args[3]))?;
    println!(
        "{} points in {:.2}s compute with {} CPU workers; results: {}",
        sweep.results.len(),
        compute_time.as_secs_f64(),
        sweep.workers_used,
        args[3]
    );
    let checks = sweep
        .results
        .iter()
        .filter(|r| r.as_ref().is_ok_and(|p| !p.metrics.quality_pass))
        .count();
    let failed = sweep.results.iter().filter(|r| r.is_err()).count();
    if checks > 0 || failed > 0 {
        eprintln!(
            "REVIEW REQUIRED: {checks} CHECK, {failed} failed; computed sweep is not validated"
        );
    } else {
        println!(
            "All points passed internal checks; this is not lab calibration or physical validation."
        );
    }
    if let Some(index) = sweep.recommended_index {
        let m = &sweep.results[index].as_ref().unwrap().metrics;
        println!(
            "Best sampled match: C1={:.6} µF, C2={:.6} µF, |Γ|={:.6}",
            m.capacitance_each_uf,
            m.capacitance_2_uf,
            m.reflection_magnitude.unwrap()
        );
    }
    if let (Some(observed), Some(compare)) =
        (&sweep.config.observed_ac, &sweep.observation_comparison)
    {
        println!(
            "Reported lab {:?} P={:+.3} W at {:.3} V RMS, C each {:.5} µF; G={:+.7} S; simulated P={:+.3} W (C each {:.5} µF)",
            observed.location,
            observed.active_power_w,
            observed.voltage_rms_v,
            observed.capacitance_each_uf,
            compare.observed_conductance_s,
            compare.simulated_active_power_w,
            compare.simulation_capacitance_each_uf
        );
        if let Some(fit) = &sweep.fitted_return {
            println!(
                "Measured-P fit: passive baseline {:+.3} W; equivalent powered AC return {:.6} A RMS at {:.1}° (not an identified physical switch)",
                fit.baseline_active_power_w, fit.fitted_current_rms_a, fit.fitted_phase_deg
            );
        }
    }
    if sweep.results.iter().any(Result::is_err) {
        eprintln!(
            "Some points did not converge. Inspect results.json and sweep.csv for per-point errors."
        );
    }
    Ok(())
}

enum Message {
    Progress(usize, usize),
    Finished(Box<Result<(Sweep, PathBuf, Duration), String>>),
}

#[derive(Clone, Copy, Debug)]
struct RunPerformance {
    compute_time: Duration,
    points: usize,
    workers: usize,
    failed: usize,
    quality_checks: usize,
}

impl RunPerformance {
    fn points_per_second(self) -> f64 {
        self.points as f64 / self.compute_time.as_secs_f64().max(1e-9)
    }

    fn summary(self) -> String {
        format!(
            "{} workers   ·   {} points   ·   {:.2}s compute   ·   {:.2} points/s   ·   {} failed   ·   {} CHECK",
            self.workers,
            self.points,
            self.compute_time.as_secs_f64(),
            self.points_per_second(),
            self.failed,
            self.quality_checks
        )
    }
}

struct LabApp {
    config: Config,
    form: [String; 12],
    reference: String,
    parent: String,
    receiver: Receiver<Message>,
    sender: Sender<Message>,
    running: bool,
    progress: (usize, usize),
    started_at: Option<Instant>,
    performance: Option<RunPerformance>,
    status: String,
    status_error: bool,
    sweep: Option<Sweep>,
    /// Incremented after each completed run so plots never reuse zoom bounds from old data.
    plot_generation: u64,
    selected: Option<usize>,
    /// Only bring a row into view when selection changes; manual scrolling stays untouched.
    table_focused: Option<(u64, usize)>,
    output: Option<PathBuf>,
    tab: usize,
    preset: Option<usize>,
    reset_pending: bool,
    document: usize,
    doc_section: usize,
    markdown_cache: CommonMarkCache,
    model_kind: usize,
    model_threshold: String,
    return_enabled: bool,
    return_current: String,
    return_phase: String,
    tank_l: String,
    tank_c: String,
    tank_g: String,
    pulse_v: String,
    pulse_f: String,
    measurement_path: String,
    measurement_frequency: String,
    invert_measured_current: bool,
    measured: Option<measurement::Measurement>,
    measurement_message: String,
    observed_enabled: bool,
    observed_location: Location,
    observed_voltage: String,
    observed_capacitance: String,
    observed_p: String,
    observed_q: String,
    fit_observed: bool,
    l_source_enabled: bool,
    l_source: String,
    cap_advanced: bool,
    cap_ratio_form: String,
    cap_esr_form: String,
    cap_leakage_form: String,
    diode_cap_form: String,
    diode_qrr_form: String,
    gate_enabled: bool,
    gate_rep_form: String,
    gate_duty_form: String,
    ring_start_v: String,
    ring_cycles: String,
    ring_clamps_disabled: bool,
    ring_result: Option<RingdownResult>,
    ring_message: String,
    discharge_result: Option<diode_plug_lab::analysis::DischargeResult>,
    discharge_message: String,
    discharge_single_cap: bool,
    /// Widget rects sampled during a frame; used by the headless layout tests.
    probe: Probe,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Default, Clone, Copy)]
struct Probe {
    input_scroll: Option<egui::Rect>,
    first_field_label: Option<egui::Rect>,
    first_field_edit: Option<egui::Rect>,
    last_input_edit: Option<egui::Rect>,
    run_button: Option<egui::Rect>,
    run_status: Option<egui::Rect>,
    tab_bar: Option<egui::Rect>,
    status_bar: Option<egui::Rect>,
    performance_line: Option<egui::Rect>,
    version_line: Option<egui::Rect>,
    point_next: Option<egui::Rect>,
    table_viewport: Option<egui::Rect>,
    table_header: Option<egui::Rect>,
    selected_table_cell: Option<egui::Rect>,
    header_summary: Option<egui::Rect>,
    header_bar: Option<egui::Rect>,
    header_cards: [Option<egui::Rect>; 3],
    reset_button: Option<egui::Rect>,
    reset_confirm: Option<egui::Rect>,
    docs_button: Option<egui::Rect>,
    doc_figure: Option<egui::Rect>,
    doc_article: Option<egui::Rect>,
    institute_link: Option<egui::Rect>,
    analysis_viewport: Option<egui::Rect>,
    sweep_plot_transforms: [Option<egui_plot::PlotTransform>; 4],
    voltage_plot_y_bounds: Option<[f64; 2]>,
    voltage_plot_transform: Option<egui_plot::PlotTransform>,
}

const LABELS: [(&str, &str); 12] = [
    ("Source voltage", "V RMS"),
    ("Source frequency", "Hz"),
    ("Load series R", "Ω"),
    ("Load series L", "H"),
    ("Load parallel C", "F"),
    ("Source series Rs", "Ω"),
    ("Diode forward Vf", "V"),
    ("Diode dynamic Rd", "Ω"),
    ("C1 minimum", "µF"),
    ("C1 maximum", "µF"),
    ("Sweep points", ""),
    ("CPU workers (0 = auto)", ""),
];

impl LabApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::new_with_ctx(&cc.egui_ctx)
    }

    fn new_with_ctx(egui_ctx: &egui::Context) -> Self {
        egui_ctx.set_visuals(egui::Visuals::dark());
        egui_extras::install_image_loaders(egui_ctx);
        let mut style = (*egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        style.visuals.panel_fill = Color32::from_rgb(17, 22, 27);
        style.visuals.extreme_bg_color = Color32::from_rgb(12, 17, 22);
        style.visuals.faint_bg_color = Color32::from_rgb(23, 32, 39);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(27, 100, 108);
        style.visuals.selection.bg_fill = Color32::from_rgb(21, 91, 103);
        egui_ctx.set_style_of(egui::Theme::Dark, style);
        let c = default_config();
        let values = [
            c.circuit.voltage_rms,
            c.circuit.frequency,
            c.circuit.resistance,
            c.circuit.inductance,
            c.circuit.load_capacitance,
            c.circuit.source_resistance,
            c.circuit.diode_drop,
            c.circuit.diode_resistance,
            c.capacitance_min_uf,
            c.capacitance_max_uf,
            c.points as f64,
            c.workers as f64,
        ];
        let (sender, receiver) = mpsc::channel();
        let source_frequency = c.circuit.frequency.to_string();
        Self {
            form: values.map(|v| v.to_string()),
            reference: c.reference_ohms.unwrap().to_string(),
            parent: "results".into(),
            config: c,
            receiver,
            sender,
            running: false,
            progress: (0, 0),
            started_at: None,
            performance: None,
            status: "Ready for a sweep".into(),
            status_error: false,
            sweep: None,
            plot_generation: 0,
            selected: None,
            table_focused: None,
            output: None,
            tab: 0,
            preset: None,
            reset_pending: false,
            document: 0,
            doc_section: 0,
            markdown_cache: CommonMarkCache::default(),
            model_kind: 0,
            model_threshold: "0V".into(),
            return_enabled: false,
            return_current: "0.1A".into(),
            return_phase: "0".into(),
            tank_l: "1mH".into(),
            tank_c: "10nF".into(),
            tank_g: "1mS".into(),
            pulse_v: "34V".into(),
            pulse_f: "50Hz".into(),
            measurement_path: String::new(),
            measurement_frequency: source_frequency,
            invert_measured_current: false,
            measured: None,
            measurement_message:
                "Import an integer number of periods of sampled port voltage and input current."
                    .into(),
            observed_enabled: false,
            observed_location: Location::Source,
            observed_voltage: "240V".into(),
            observed_capacitance: "10uF".into(),
            observed_p: "-29W".into(),
            observed_q: String::new(),
            fit_observed: false,
            l_source_enabled: false,
            l_source: "0H".into(),
            cap_advanced: false,
            cap_ratio_form: "1".into(),
            cap_esr_form: "0".into(),
            cap_leakage_form: String::new(),
            diode_cap_form: "0".into(),
            diode_qrr_form: "0".into(),
            gate_enabled: false,
            gate_rep_form: "50Hz".into(),
            gate_duty_form: "0.5".into(),
            ring_start_v: "10V".into(),
            ring_cycles: "20".into(),
            ring_clamps_disabled: true,
            ring_result: None,
            ring_message: "Set tank L, C and loss G on this tab, then run a ringdown probe.".into(),
            discharge_result: None,
            discharge_message:
                "Select a sweep point, then choose an isolated single- or two-capacitor discharge."
                    .into(),
            discharge_single_cap: true,
            probe: Probe::default(),
        }
    }

    fn apply_preset(&mut self, index: usize) {
        let presets = presets::all();
        let Some(preset) = presets.get(index) else {
            return;
        };
        let c = &preset.config;
        let values = [
            c.circuit.voltage_rms,
            c.circuit.frequency,
            c.circuit.resistance,
            c.circuit.inductance,
            c.circuit.load_capacitance,
            c.circuit.source_resistance,
            c.circuit.diode_drop,
            c.circuit.diode_resistance,
            c.capacitance_min_uf,
            c.capacitance_max_uf,
            c.points as f64,
            self.config.workers as f64,
        ];
        self.form = values.map(|v| v.to_string());
        self.reference = c.reference_ohms.map_or(String::new(), |z| z.to_string());
        match c.circuit.load_model {
            LoadModel::Linear => {
                self.model_kind = 0;
                self.model_threshold = "0V".into();
            }
            LoadModel::Led { knee_voltage } => {
                self.model_kind = 1;
                self.model_threshold = format!("{knee_voltage}V");
            }
            LoadModel::Battery {
                open_circuit_voltage,
            } => {
                self.model_kind = 2;
                self.model_threshold = format!("{open_circuit_voltage}V");
            }
            LoadModel::BidirectionalBus { emf_voltage } => {
                self.model_kind = 3;
                self.model_threshold = format!("{emf_voltage}V");
            }
        }
        self.return_enabled = c.circuit.ac_return.is_some();
        if let Some(ret) = &c.circuit.ac_return {
            self.return_current = format!("{}A", ret.current_rms_a);
            self.return_phase = ret.phase_deg.to_string();
        }
        self.l_source_enabled = c.circuit.source_inductance > 0.0;
        self.l_source = format!("{}H", c.circuit.source_inductance);
        self.cap_advanced = c.circuit.capacitor_ratio != 1.0
            || c.circuit.capacitor_esr_ohm != 0.0
            || c.circuit.diode_capacitance_f != 0.0
            || c.circuit.diode_recovery_charge_nc != 0.0
            || c.circuit.capacitor_leakage_parallel_ohm.is_some();
        self.cap_ratio_form = format!("{}", c.circuit.capacitor_ratio);
        self.cap_esr_form = format!("{}mOhm", c.circuit.capacitor_esr_ohm * 1e3);
        self.cap_leakage_form = c
            .circuit
            .capacitor_leakage_parallel_ohm
            .map_or(String::new(), |r| format!("{}MegOhm", r / 1e6));
        self.diode_cap_form = format!("{}pF", c.circuit.diode_capacitance_f * 1e12);
        self.diode_qrr_form = format!("{}", c.circuit.diode_recovery_charge_nc);
        self.gate_enabled = c.circuit.pulse_gate.is_some();
        if let Some(gate) = &c.circuit.pulse_gate {
            self.gate_rep_form = format!("{}Hz", gate.repetition_hz);
            self.gate_duty_form = format!("{}", gate.duty);
        }
        self.observed_enabled = c.observed_ac.is_some();
        self.fit_observed = c.fit_to_observed_power;
        if let Some(observed) = &c.observed_ac {
            self.observed_location = observed.location;
            self.observed_voltage = format!("{}V", observed.voltage_rms_v);
            self.observed_capacitance = format!("{}uF", observed.capacitance_each_uf);
            self.observed_p = format!("{}W", observed.active_power_w);
            self.observed_q = observed
                .reactive_power_var
                .map_or(String::new(), |q| format!("{q}var"));
        }
        self.preset = Some(index);
        self.measurement_frequency = c.circuit.frequency.to_string();
        self.config.preset_name = Some(preset.name.to_owned());
        self.status = format!("Loaded: {} · edit any value before running", preset.name);
        self.status_error = false;
    }

    /// Restore the editable scenario without discarding previously computed results.
    fn reset_inputs(&mut self) {
        if self.running {
            return;
        }
        let c = default_config();
        self.form = [
            c.circuit.voltage_rms,
            c.circuit.frequency,
            c.circuit.resistance,
            c.circuit.inductance,
            c.circuit.load_capacitance,
            c.circuit.source_resistance,
            c.circuit.diode_drop,
            c.circuit.diode_resistance,
            c.capacitance_min_uf,
            c.capacitance_max_uf,
            c.points as f64,
            c.workers as f64,
        ]
        .map(|value| value.to_string());
        self.reference = c.reference_ohms.unwrap().to_string();
        self.measurement_path.clear();
        self.measurement_frequency = c.circuit.frequency.to_string();
        self.invert_measured_current = false;
        self.config = c;
        self.parent = "results".into();
        self.preset = None;
        self.model_kind = 0;
        self.model_threshold = "0V".into();
        self.return_enabled = false;
        self.return_current = "0.1A".into();
        self.return_phase = "0".into();
        self.l_source_enabled = false;
        self.l_source = "0H".into();
        self.cap_advanced = false;
        self.cap_ratio_form = "1".into();
        self.cap_esr_form = "0".into();
        self.cap_leakage_form.clear();
        self.diode_cap_form = "0".into();
        self.diode_qrr_form = "0".into();
        self.gate_enabled = false;
        self.gate_rep_form = "50Hz".into();
        self.gate_duty_form = "0.5".into();
        self.tank_l = "1mH".into();
        self.tank_c = "10nF".into();
        self.tank_g = "1mS".into();
        self.pulse_v = "34V".into();
        self.pulse_f = "50Hz".into();
        self.ring_start_v = "10V".into();
        self.ring_cycles = "20".into();
        self.ring_clamps_disabled = true;
        self.discharge_single_cap = true;
        self.observed_enabled = false;
        self.observed_location = Location::Source;
        self.observed_voltage = "240V".into();
        self.observed_capacitance = "10uF".into();
        self.observed_p = "-29W".into();
        self.observed_q.clear();
        self.fit_observed = false;
        self.reset_pending = false;
        self.status = "Inputs reset to defaults · previous results retained".into();
        self.status_error = false;
    }

    fn documentation(&mut self, ui: &mut egui::Ui) {
        let reading_width = ui.available_width().min(1160.0);
        let left_margin = ((ui.available_width() - reading_width) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(left_margin);
            let article = ui.vertical(|ui| {
                ui.set_width(reading_width);
                self.documentation_article(ui);
            });
            #[cfg(test)]
            {
                self.probe.doc_article = Some(article.response.rect);
            }
            #[cfg(not(test))]
            let _ = article;
        });
    }

    fn documentation_article(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Technical library").size(26.0).color(TEAL));
        ui.label(
            "Circuit theory and an explicit account of what the simulator does—and does not—model.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            for (index, (name, _)) in DOCUMENTS.iter().enumerate() {
                if ui.selectable_label(self.document == index, *name).clicked()
                    && self.document != index
                {
                    self.document = index;
                    self.doc_section = 0;
                }
            }
        });
        ui.add_space(8.0);
        ui.separator();
        if self.document == 0 {
            egui::Frame::new()
                .fill(Color32::from_rgb(53, 44, 36))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new("Reading note · original exploratory material").color(ORANGE).strong());
                    ui.label("For validated sign conventions and model boundaries, consult Model coverage. Equations retain the source document's notation; figures without supplied assets are identified in place.");
                });
            ui.add_space(16.0);
        }
        let sections = &DOCUMENT_SECTIONS[self.document];
        let current = self.doc_section.min(sections.len() - 1);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("SECTION").color(TEAL).strong());
            egui::ComboBox::from_id_salt(("document_section", self.document))
                .width(ui.available_width().min(600.0).max(140.0))
                .selected_text(&sections[current].title)
                .show_ui(ui, |ui| {
                    for (index, section) in sections.iter().enumerate() {
                        ui.selectable_value(&mut self.doc_section, index, &section.title);
                    }
                });
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.doc_section > 0, egui::Button::new("Previous"))
                .clicked()
            {
                self.doc_section -= 1;
            }
            ui.small(format!("{} / {}", self.doc_section + 1, sections.len()));
            if ui
                .add_enabled(
                    self.doc_section + 1 < sections.len(),
                    egui::Button::new("Next"),
                )
                .clicked()
            {
                self.doc_section += 1;
            }
        });
        ui.add_space(18.0);
        let mut style = ui.style().as_ref().clone();
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(17.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(30.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(16.0));
        style.spacing.item_spacing = egui::vec2(12.0, 14.0);
        ui.scope(|ui| {
            ui.set_style(style);
            egui::Frame::new()
                .fill(Color32::from_rgb(27, 36, 45))
                .corner_radius(9)
                .inner_margin(egui::Margin::symmetric(30, 26))
                .show(ui, |ui| {
                    let section = &sections[self.doc_section.min(sections.len() - 1)];
                    if self.document == 1 && self.doc_section == 0 {
                        ui.label("Scroll sideways to inspect the full model-coverage table.");
                        egui::ScrollArea::horizontal()
                            .id_salt("coverage_table_scroll")
                            .max_width(ui.available_width())
                            .show(ui, |ui| {
                                ui.set_min_width(1100.0);
                                CommonMarkViewer::new()
                                    .render_math_fn(Some(&render_equation))
                                    .show(ui, &mut self.markdown_cache, &section.markdown);
                            });
                    } else if let Some((before, after)) =
                        section.markdown.split_once(DIODE_FIGURE_MARKER)
                    {
                        CommonMarkViewer::new()
                            .render_math_fn(Some(&render_equation))
                            .show(ui, &mut self.markdown_cache, before);
                        self.diagram(ui);
                        CommonMarkViewer::new()
                            .render_math_fn(Some(&render_equation))
                            .show(ui, &mut self.markdown_cache, after);
                    } else {
                        CommonMarkViewer::new()
                            .render_math_fn(Some(&render_equation))
                            .show(ui, &mut self.markdown_cache, &section.markdown);
                    }
                });
        });
    }

    fn diagram(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(246, 247, 245))
                .corner_radius(7)
                .inner_margin(egui::Margin::same(24))
                .show(ui, |ui| {
                    let image = ui.add(
                        egui::Image::from_bytes("bytes://diode-plug-simple.svg", DIODE_FIGURE)
                            .fit_to_original_size(1.0)
                            .max_width(ui.available_width().min(600.0)),
                    );
                    // draw.io's embedded bitmap labels are not rendered by usvg.
                    // Put those same labels on the diagram with egui's bundled font.
                    let scale = image.rect.width() / 491.0;
                    for (label, x, y) in [
                        ("A", 258.0, 26.0),
                        ("B", 260.0, 344.0),
                        ("P", 173.0, 174.0),
                        ("N", 467.0, 174.0),
                        ("C1", 184.0, 112.0),
                        ("C2", 474.0, 112.0),
                        ("D1", 184.0, 262.0),
                        ("D2", 474.0, 262.0),
                    ] {
                        ui.painter().text(
                            image.rect.min + egui::vec2(x * scale, y * scale),
                            egui::Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(15.0 * scale.max(0.75)),
                            Color32::from_rgb(28, 48, 58),
                        );
                    }
                    #[cfg(test)]
                    {
                        self.probe.doc_figure = Some(image.rect);
                    }
                    #[cfg(not(test))]
                    let _ = image;
                });
            ui.add_space(5.0);
            ui.label(
                RichText::new("Fig. 1 · Diode-plug circuit · connect the load across P–N")
                    .size(14.0)
                    .color(Color32::from_gray(170)),
            );
        });
        ui.add_space(18.0);
    }

    fn read_config(&self) -> Result<Config, String> {
        let mut v = [0.0; 12];
        let units = [
            Unit::Volt,
            Unit::Hertz,
            Unit::Ohm,
            Unit::Henry,
            Unit::Farad,
            Unit::Ohm,
            Unit::Volt,
            Unit::Ohm,
            Unit::Farad,
            Unit::Farad,
        ];
        for i in 0..10 {
            let scale = if i == 8 || i == 9 { 1e-6 } else { 1.0 };
            let value = units::parse(&self.form[i], units[i], scale)
                .map_err(|e| format!("{}: {e}", LABELS[i].0))?;
            v[i] = if i == 8 || i == 9 {
                value / 1e-6
            } else {
                value
            };
        }
        let points = self.form[10]
            .trim()
            .parse::<usize>()
            .map_err(|_| "Sweep points must be an integer")?;
        let workers = self.form[11]
            .trim()
            .parse::<usize>()
            .map_err(|_| "Workers must be a nonnegative integer")?;
        let reference = if self.reference.trim().is_empty() {
            None
        } else {
            Some(
                units::parse(&self.reference, Unit::Ohm, 1.0)
                    .map_err(|e| format!("Reference Z0: {e}"))?,
            )
        };
        let load_model = match self.model_kind {
            0 => LoadModel::Linear,
            1 => LoadModel::Led {
                knee_voltage: units::parse(&self.model_threshold, Unit::Volt, 1.0)
                    .map_err(|e| format!("LED knee: {e}"))?,
            },
            2 => LoadModel::Battery {
                open_circuit_voltage: units::parse(&self.model_threshold, Unit::Volt, 1.0)
                    .map_err(|e| format!("Battery EMF: {e}"))?,
            },
            _ => LoadModel::BidirectionalBus {
                emf_voltage: units::parse(&self.model_threshold, Unit::Volt, 1.0)
                    .map_err(|e| format!("Powered DC bus EMF: {e}"))?,
            },
        };
        let ac_return = if self.return_enabled {
            Some(AcReturn {
                current_rms_a: units::parse(&self.return_current, Unit::Ampere, 1.0)
                    .map_err(|e| format!("Return path current: {e}"))?,
                phase_deg: self
                    .return_phase
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "Return phase must be degrees (a finite number)")?,
            })
        } else {
            None
        };
        let observed_ac = if self.observed_enabled {
            Some(ObservedAc {
                location: self.observed_location,
                voltage_rms_v: units::parse(&self.observed_voltage, Unit::Volt, 1.0)
                    .map_err(|e| format!("Observed voltage: {e}"))?,
                capacitance_each_uf: units::parse(&self.observed_capacitance, Unit::Farad, 1e-6)
                    .map_err(|e| format!("Observed C each: {e}"))?
                    / 1e-6,
                active_power_w: units::parse(&self.observed_p, Unit::Watt, 1.0)
                    .map_err(|e| format!("Observed P: {e}"))?,
                reactive_power_var: if self.observed_q.trim().is_empty() {
                    None
                } else {
                    Some(
                        units::parse(&self.observed_q, Unit::Var, 1.0)
                            .map_err(|e| format!("Observed Q: {e}"))?,
                    )
                },
                note: self
                    .config
                    .observed_ac
                    .as_ref()
                    .map_or(String::new(), |o| o.note.clone()),
            })
        } else {
            None
        };
        let config = Config {
            circuit: Circuit {
                voltage_rms: v[0],
                frequency: v[1],
                resistance: v[2],
                inductance: v[3],
                load_capacitance: v[4],
                source_resistance: v[5],
                diode_drop: v[6],
                diode_resistance: v[7],
                load_model,
                ac_return,
                source_inductance: if self.l_source_enabled {
                    units::parse(&self.l_source, Unit::Henry, 1.0)
                        .map_err(|e| format!("Ls: {e}"))?
                } else {
                    0.0
                },
                capacitor_ratio: if self.cap_advanced {
                    units::parse(&self.cap_ratio_form, Unit::Unitless, 1.0)
                        .map_err(|e| format!("C2 ratio: {e}"))?
                } else {
                    1.0
                },
                capacitor_esr_ohm: if self.cap_advanced {
                    units::parse(&self.cap_esr_form, Unit::Ohm, 1.0)
                        .map_err(|e| format!("ESR: {e}"))?
                } else {
                    0.0
                },
                capacitor_leakage_parallel_ohm: if self.cap_advanced
                    && !self.cap_leakage_form.trim().is_empty()
                {
                    Some(
                        units::parse(&self.cap_leakage_form, Unit::Ohm, 1.0)
                            .map_err(|e| format!("Leakage: {e}"))?,
                    )
                } else {
                    None
                },
                diode_capacitance_f: if self.cap_advanced {
                    units::parse(&self.diode_cap_form, Unit::Farad, 1.0)
                        .map_err(|e| format!("Diode Cj: {e}"))?
                } else {
                    0.0
                },
                diode_recovery_charge_nc: if self.cap_advanced {
                    units::parse(&self.diode_qrr_form, Unit::Unitless, 1.0)
                        .map_err(|e| format!("Qrr: {e}"))?
                } else {
                    0.0
                },
                pulse_gate: if self.gate_enabled {
                    Some(PulseGate {
                        repetition_hz: units::parse(&self.gate_rep_form, Unit::Hertz, 1.0)
                            .map_err(|e| format!("Gate repetition: {e}"))?,
                        duty: units::parse(&self.gate_duty_form, Unit::Unitless, 1.0)
                            .map_err(|e| format!("Gate duty: {e}"))?,
                    })
                } else {
                    None
                },
            },
            preset_name: self.config.preset_name.clone(),
            capacitance_min_uf: v[8],
            capacitance_max_uf: v[9],
            points,
            workers,
            reference_ohms: reference,
            observed_ac,
            fit_to_observed_power: self.observed_enabled && self.fit_observed,
            operator: self.config.operator.clone(),
            run_id: self.config.run_id.clone(),
        };
        config.validate()?;
        Ok(config)
    }

    fn start(&mut self, ctx: &egui::Context) {
        let config = match self.read_config() {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                self.status_error = true;
                return;
            }
        };
        if self.parent.trim().is_empty() {
            self.status = "Enter a results directory".into();
            self.status_error = true;
            return;
        }
        self.config = config.clone();
        self.sweep = None;
        self.output = None;
        self.selected = None;
        self.progress = (0, config.points);
        self.running = true;
        self.started_at = Some(Instant::now());
        self.performance = None;
        self.status = format!("Computing with up to {} CPU workers…", config.workers());
        self.status_error = false;
        let tx = self.sender.clone();
        let redraw = ctx.clone();
        let parent = PathBuf::from(self.parent.trim());
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_ctx = redraw.clone();
            let started = Instant::now();
            let computed = sweep::run(config, move |done, total| {
                let _ = progress_tx.send(Message::Progress(done, total));
                progress_ctx.request_repaint();
            });
            let compute_time = started.elapsed();
            let result = computed.and_then(|sweep| {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_millis();
                let name: String = sweep
                    .config
                    .run_id
                    .chars()
                    .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
                    .take(36)
                    .collect();
                let folder = parent.join(format!(
                    "run-{stamp}{}",
                    if name.is_empty() {
                        String::new()
                    } else {
                        format!("-{name}")
                    }
                ));
                export::write(&sweep, &folder)?;
                Ok((sweep, folder, compute_time))
            });
            let _ = tx.send(Message::Finished(Box::new(result)));
            redraw.request_repaint();
        });
    }

    fn selected_result(&self) -> Option<&ResultPoint> {
        self.selected
            .and_then(|i| self.sweep.as_ref()?.results.get(i)?.as_ref().ok())
    }

    /// Separate waveform zoom state for every run and selected capacitance point.
    fn trace_plot_id(&self, name: &'static str) -> (&'static str, u64, Option<usize>) {
        (name, self.plot_generation, self.selected)
    }

    fn point_selector(&mut self, ui: &mut egui::Ui) {
        let Some(sweep) = &self.sweep else {
            ui.add_enabled(false, egui::Button::new("Point: run a sweep"));
            return;
        };
        // Keep real sweep indices (failures remain in the table), but never select a failed solve.
        let points: Vec<(usize, String)> = sweep
            .results
            .iter()
            .enumerate()
            .filter_map(|(i, result)| {
                result.as_ref().ok().map(|r| {
                    let m = &r.metrics;
                    (
                        i,
                        format!(
                            "#{:02} · C1 {:.5} · C2 {:.5} µF{}",
                            i + 1,
                            m.capacitance_each_uf,
                            m.capacitance_2_uf,
                            if m.quality_pass { "" } else { " · CHECK" }
                        ),
                    )
                })
            })
            .collect();
        if points.is_empty() {
            ui.add_enabled(false, egui::Button::new("No solved points"));
            return;
        }
        let solved_indices: Vec<usize> = points.iter().map(|(i, _)| *i).collect();
        let previous = adjacent_solved(&solved_indices, self.selected, -1);
        let following = adjacent_solved(&solved_indices, self.selected, 1);
        ui.label("Point");
        if ui
            .add_enabled(previous.is_some(), egui::Button::new("<"))
            .on_hover_text("Previous solved capacitance point")
            .clicked()
        {
            self.selected = previous;
        }
        let current = self.selected;
        egui::ComboBox::from_id_salt("selected_capacitance_point")
            .width(210.0)
            .selected_text(
                points
                    .iter()
                    .find(|(i, _)| Some(*i) == current)
                    .map_or("Choose a solved point", |(_, label)| label.as_str()),
            )
            .show_ui(ui, |ui| {
                for (i, label) in &points {
                    ui.selectable_value(&mut self.selected, Some(*i), label);
                }
            });
        let next_response = ui
            .add_enabled(following.is_some(), egui::Button::new(">"))
            .on_hover_text("Next solved capacitance point");
        #[cfg(test)]
        {
            self.probe.point_next = Some(next_response.rect);
        }
        if next_response.clicked() {
            self.selected = following;
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        {
            self.probe.status_bar = Some(ui.max_rect());
        }
        ui.horizontal(|ui| {
            let indicator = if self.status_error {
                ORANGE
            } else if self.running {
                ORANGE
            } else if self.performance.is_some() {
                TEAL
            } else {
                BLUE
            };
            let (lamp, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), egui::Sense::hover());
            ui.painter().circle_filled(lamp.center(), 4.0, indicator);
            let version = format!("v{}", env!("CARGO_PKG_VERSION"));
            let status_width = (ui.available_width() * 0.24).clamp(95.0, 245.0);
            let response = ui
                .add_sized(
                    [status_width, 18.0],
                    egui::Label::new(RichText::new(&self.status).color(if self.status_error {
                        ORANGE
                    } else {
                        Color32::LIGHT_GRAY
                    }))
                    .truncate(),
                )
                .on_hover_text(self.output.as_ref().map_or_else(
                    || self.status.clone(),
                    |path| format!("{}\nResults: {}", self.status, path.display()),
                ));
            self.probe.run_status = Some(response.rect);
            let (fraction, progress_text, performance) = if self.running {
                let elapsed = self
                    .started_at
                    .map_or(0.0, |start| start.elapsed().as_secs_f64());
                (
                    self.progress.0 as f32 / self.progress.1.max(1) as f32,
                    format!("{} / {}", self.progress.0, self.progress.1),
                    if self.progress.0 == self.progress.1 {
                        "All points computed · writing results (single thread)".into()
                    } else {
                        format!(
                            "up to {} workers · {:.1}s · {:.2} points/s so far",
                            self.config.workers(),
                            elapsed,
                            self.progress.0 as f64 / elapsed.max(1e-9)
                        )
                    },
                )
            } else if let Some(last) = self.performance {
                (
                    (last.points - last.failed) as f32 / last.points.max(1) as f32,
                    format!("{} / {}", last.points - last.failed, last.points),
                    last.summary(),
                )
            } else {
                (
                    0.0,
                    "Idle".into(),
                    format!(
                        "Idle · {} logical CPUs available · set 0 for automatic selection",
                        std::thread::available_parallelism().map_or(1, |n| n.get())
                    ),
                )
            };
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(105.0)
                    .text(progress_text),
            );
            let performance_width = (ui.available_width() - 65.0).max(20.0);
            let line = ui
                .add_sized(
                    [performance_width, 18.0],
                    egui::Label::new(&performance).truncate(),
                )
                .on_hover_text(&performance);
            #[cfg(test)]
            {
                self.probe.performance_line = Some(line.rect);
            }
            #[cfg(not(test))]
            let _ = line;
            let version_response = ui.strong(RichText::new(version).color(TEAL));
            #[cfg(test)]
            {
                self.probe.version_line = Some(version_response.rect);
            }
            #[cfg(not(test))]
            let _ = version_response;
        });
        if self.running {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }

    fn input_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Configuration").size(18.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let reset = ui
                    .add_enabled(
                        !self.running,
                        egui::Button::new("Reset to defaults").frame(false),
                    )
                    .on_hover_text(
                        "Restore all editable circuit and run inputs. Completed results are kept.",
                    );
                if reset.clicked() {
                    self.reset_pending = true;
                }
                #[cfg(test)]
                {
                    self.probe.reset_button = Some(reset.rect);
                }
            });
        });
        if self.reset_pending {
            ui.horizontal_wrapped(|ui| {
                ui.small("Reset inputs? Results stay available.");
                let confirm = ui.add_enabled(!self.running, egui::Button::new("Reset now").small());
                if confirm.clicked() {
                    self.reset_inputs();
                }
                #[cfg(test)]
                {
                    self.probe.reset_confirm = Some(confirm.rect);
                }
                if ui.small_button("Cancel").clicked() {
                    self.reset_pending = false;
                }
            });
        }
        ui.small("Select a fixture, then adjust the circuit and sweep.");
        ui.separator();
        // Only the editable form scrolls; the primary action stays in the header.
        let _out = egui::ScrollArea::vertical()
            .id_salt("setup_form")
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink([false, false])
            .show(ui, |ui| {
            let presets = presets::all();
            let mut choice = self.preset;
            egui::ComboBox::from_label("Lab preset")
                .selected_text(
                    choice
                        .and_then(|i| presets.get(i).map(|p| p.name))
                        .unwrap_or("Custom / select example"),
                )
                .width(275.0)
                .show_ui(ui, |ui| {
                    for (i, p) in presets.iter().enumerate() {
                        ui.selectable_value(&mut choice, Some(i), p.name);
                    }
                });
            if choice != self.preset
                && let Some(i) = choice
            {
                self.apply_preset(i);
            }
            if let Some(i) = self.preset {
                ui.label(RichText::new(presets[i].note).small().color(ORANGE));
            }
            ui.add_space(8.0);
            ui.heading(RichText::new("Source, load & diodes").size(16.0));
            for i in 0..8 {
                self.field(ui, i);
            }
            egui::ComboBox::from_label("Load branch model")
                .selected_text(match self.model_kind {
                    0 => "Linear R–L || C",
                    1 => "LED knee + R || C",
                    2 => "Battery charge only + R || C",
                    _ => "Bidirectional DC bus + R || C",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.model_kind, 0, "Linear R–L || C");
                    ui.selectable_value(&mut self.model_kind, 1, "LED knee + R || C (blocking)");
                    ui.selectable_value(
                        &mut self.model_kind,
                        2,
                        "Battery EMF + R || C (charging only)",
                    );
                    ui.selectable_value(&mut self.model_kind, 3, "Bidirectional DC bus + R || C");
                });
            if self.model_kind != 0 {
                ui.horizontal(|ui| {
                    ui.label(match self.model_kind { 1 => "LED knee", 2 => "Battery EMF", _ => "DC bus EMF" });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.model_threshold).desired_width(100.0),
                    );
                    ui.label("V");
                });
                ui.small(if self.model_kind == 3 {
                    "Bidirectional DC source: current may reverse; set L = 0 H. DC bus alone need not make AC G negative."
                } else { "Ideal reverse-current blocking; set L = 0 H for this model." });
            }
            ui.add_space(8.0);
            ui.checkbox(&mut self.l_source_enabled, "Source-side series inductance Ls");
            if self.l_source_enabled {
                ui.horizontal(|ui| {
                    ui.label("Ls");
                    ui.add(egui::TextEdit::singleline(&mut self.l_source).desired_width(78.0));
                    ui.label("H");
                });
                ui.small("Requires junction capacitance Cj > 0; clamp-node transient becomes dynamic.");
            }
            ui.checkbox(&mut self.cap_advanced, "Advanced capacitor & diode parasitics");
            if self.cap_advanced {
                ui.horizontal(|ui| {
                    ui.label("C2 ratio");
                    ui.add(egui::TextEdit::singleline(&mut self.cap_ratio_form).desired_width(78.0));
                    ui.small("C2 = ratio·C1");
                });
                ui.horizontal(|ui| {
                    ui.label("ESR");
                    ui.add(egui::TextEdit::singleline(&mut self.cap_esr_form).desired_width(78.0));
                    ui.label("Ω");
                });
                ui.horizontal(|ui| {
                    ui.label("Leakage");
                    ui.add(egui::TextEdit::singleline(&mut self.cap_leakage_form).desired_width(78.0));
                    ui.label("Ω (blank = none)");
                });
                ui.horizontal(|ui| {
                    ui.label("Diode Cj");
                    ui.add(egui::TextEdit::singleline(&mut self.diode_cap_form).desired_width(78.0));
                    ui.label("F");
                });
                ui.horizontal(|ui| {
                    ui.label("Qrr");
                    ui.add(egui::TextEdit::singleline(&mut self.diode_qrr_form).desired_width(78.0));
                    ui.label("nC");
                });
            }
            ui.checkbox(&mut self.gate_enabled, "Pulsed (gated) load");
            if self.gate_enabled {
                ui.horizontal(|ui| {
                    ui.label("Repetition");
                    ui.add(egui::TextEdit::singleline(&mut self.gate_rep_form).desired_width(78.0));
                    ui.label("Hz");
                });
                ui.horizontal(|ui| {
                    ui.label("Duty");
                    ui.add(egui::TextEdit::singleline(&mut self.gate_duty_form).desired_width(78.0));
                    ui.label("0–1");
                });
                ui.small("Gating requires L = 0 H; phase-locked square wave at t = 0.");
            }
            ui.checkbox(&mut self.return_enabled, "External powered AC return (at A–B)");
            if self.return_enabled {
                ui.horizontal(|ui| {
                    ui.label("Injected current");
                    ui.add(egui::TextEdit::singleline(&mut self.return_current).desired_width(78.0));
                    ui.label("RMS");
                });
                ui.horizontal(|ui| {
                    ui.label("Phase vs source sine");
                    ui.add(egui::TextEdit::singleline(&mut self.return_phase).desired_width(78.0));
                    ui.label("degrees");
                });
                ui.label(RichText::new("External powered switched-return equivalent; reported total G includes this driver. Plug-only G is listed separately.").small().color(ORANGE));
            }
            ui.small("SI suffixes accepted: 5uH · 10pF · 1mOhm · 2.4kΩ");
            ui.add_space(10.0);
            ui.heading(RichText::new("Capacitance sweep").size(16.0));
            ui.small("C2 follows C1 (equal unless a ratio is set under parasitics).");
            for i in 8..12 {
                self.field(ui, i);
            }
            ui.horizontal(|ui| {
                ui.label("Reference Z0");
                ui.add(egui::TextEdit::singleline(&mut self.reference).desired_width(100.0));
                ui.label("Ω (optional)");
            });
            ui.add_space(10.0);
            ui.checkbox(&mut self.observed_enabled, "Compare with reported lab AC power");
            if self.observed_enabled {
                egui::ComboBox::from_label("Meter location")
                    .selected_text(if self.observed_location == Location::Source { "Grid/source" } else { "Plug port A–B" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.observed_location, Location::Source, "Grid/source Vs");
                        ui.selectable_value(&mut self.observed_location, Location::Port, "Plug port A–B");
                    });
                ui.horizontal(|ui| {
                    ui.label("V RMS"); ui.add(egui::TextEdit::singleline(&mut self.observed_voltage).desired_width(77.0));
                    ui.label("C each"); ui.add(egui::TextEdit::singleline(&mut self.observed_capacitance).desired_width(77.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Active P"); ui.add(egui::TextEdit::singleline(&mut self.observed_p).desired_width(77.0));
                    ui.label("Reactive Q"); ui.add(egui::TextEdit::singleline(&mut self.observed_q).desired_width(77.0));
                });
                ui.small("Q blank = unknown; P<0 alone does not establish quadrant 3. Lab data is compared, not forced into the circuit model.");
                ui.checkbox(&mut self.fit_observed, "Fit equivalent powered AC return to P");
                if self.fit_observed { ui.label(RichText::new("Fits ONE observed watt value via external AC-port current injection; baseline passive result retained. Does not identify the actual return mechanism or fit unknown Q.").small().color(ORANGE)); }
            }
            ui.add_space(10.0);
            ui.heading(RichText::new("Save & identify this run").size(16.0));
            ui.small("Optional labels go into results.json. The run label is also appended to the timestamped folder name.");
            ui.horizontal(|ui| {
                ui.label("Operator").on_hover_text("Optional name included in the saved run record; not used in calculations.");
                ui.add(egui::TextEdit::singleline(&mut self.config.operator).hint_text("Optional name"));
            });
            ui.horizontal(|ui| {
                ui.label("Run label").on_hover_text("Optional sample ID used in results.json and the output folder name (letters, digits, - and _).");
                ui.add(egui::TextEdit::singleline(&mut self.config.run_id).hint_text("e.g. bench-01"));
            });
            ui.label("Save runs under").on_hover_text("Each sweep gets a new timestamped subfolder here. Existing results are never overwritten.");
            let _edit = ui.add(egui::TextEdit::singleline(&mut self.parent).hint_text("results"));
            ui.small("Each sweep creates a new folder here; existing results remain untouched.");
            #[cfg(test)]
            {
                self.probe.last_input_edit = Some(_edit.rect);
            }
            ui.add_space(24.0);
        });
        #[cfg(test)]
        {
            self.probe.input_scroll = Some(_out.inner_rect);
        }
    }

    fn run_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let run = ui.add_enabled(
            !self.running,
            egui::Button::new(
                RichText::new(if self.running { "Running…" } else { "Run sweep" })
                    .color(Color32::WHITE)
                    .strong(),
            )
            .fill(Color32::from_rgb(38, 109, 108))
            .min_size(egui::vec2(110.0, 26.0)),
        ).on_hover_text("Run the current configuration and save a new result. Numerical checks do not validate hardware.");
        if run.clicked() {
            self.start(ctx);
        }
        self.probe.run_button = Some(run.rect);
    }

    fn field(&mut self, ui: &mut egui::Ui, i: usize) {
        ui.horizontal(|ui| {
            let label = ui.add_sized(
                [158.0, 22.0],
                egui::Label::new(LABELS[i].0).halign(egui::Align::Min),
            );
            let edit = ui.add(egui::TextEdit::singleline(&mut self.form[i]).desired_width(112.0));
            #[cfg(test)]
            if i == 0 {
                self.probe.first_field_label = Some(label.rect);
                self.probe.first_field_edit = Some(edit.rect);
            }
            #[cfg(not(test))]
            let _ = (label, edit);
            ui.small(LABELS[i].1);
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        let Some(sweep) = &self.sweep else {
            ui.add_space(24.0);
            ui.heading("Ready to explore a sweep");
            ui.add_space(8.0);
            ui.label("1. Choose a lab preset or edit the configuration on the left.");
            ui.label("2. Set the capacitance range, number of points, and save location.");
            ui.label("3. Select Run sweep in the header above.");
            ui.add_space(12.0);
            ui.label(RichText::new("Results appear here and in a new saved folder. Numerical checks are not hardware validation.")
                .color(TEAL));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} points  ·  {} CPU workers",
                    sweep.results.len(),
                    sweep.workers_used
                ))
                .color(TEAL)
                .strong(),
            );
            if let Some(i) = sweep.recommended_index {
                let m = &sweep.results[i].as_ref().unwrap().metrics;
                ui.label(format!(
                    "Best sampled match: {:.5} µF each   |Γ| {:.5}   load {:.3} W",
                    m.capacitance_each_uf,
                    m.reflection_magnitude.unwrap(),
                    m.load_power_w
                ));
            } else if sweep.config.reference_ohms.is_none() {
                ui.label("No Z0 supplied: no matching recommendation.");
            } else {
                ui.label("No qualified matching recommendation (check quality).");
            }
        });
        if sweep.effective_circuit.ac_return.is_some() {
            ui.label(RichText::new("EXTERNALLY POWERED AC RETURN IS ON · G and |Γ| describe plug + powered return together, not the passive plug alone.").color(ORANGE));
        }
        if let Some(m) = self.selected_result().map(|r| &r.metrics) {
            ui.label(RichText::new(format!("Selected AC port: G total = {:+.6} S · G plug-only = {:+.6} S · P₁ = {:+.5} W · Rparallel = {}",
                m.input_admittance_real_s, m.plug_only_admittance_real_s, m.fundamental_active_power_w,
                m.equivalent_parallel_resistance_ohm.map_or("undefined".into(), |r| format!("{r:.4} Ω"))))
                .color(if m.input_admittance_real_s < 0.0 { ORANGE } else { TEAL }));
        }
        if let (Some(observed), Some(comparison)) =
            (&sweep.config.observed_ac, &sweep.observation_comparison)
        {
            ui.separator();
            ui.label(RichText::new(format!("REPORTED LAB: P = {:+.3} W at {:.3} V RMS, C1=C2={:.5} µF · inferred G = {:+.7} S, Rparallel = {}",
                observed.active_power_w, observed.voltage_rms_v, observed.capacitance_each_uf,
                comparison.observed_conductance_s,
                comparison.observed_parallel_resistance_ohm.map_or("undefined".into(), |r| format!("{r:.3} Ω"))))
                .color(ORANGE).strong());
            ui.label(format!("Circuit model at {:.5} µF: P = {:+.3} W, G = {:+.7} S · measured − model gap = {:+.3} W (in-phase current gap {:+.4} A RMS)",
                comparison.simulation_capacitance_each_uf, comparison.simulated_active_power_w,
                comparison.simulated_conductance_s, comparison.active_power_gap_w,
                comparison.in_phase_current_gap_a));
            ui.label(match (observed.reactive_power_var, comparison.observed_quadrant) {
                (Some(q), Some(quadrant)) => format!("Measured Q = {q:+.3} var · quadrant {quadrant} · PF = {}",
                    comparison.observed_signed_power_factor.map_or("unknown".into(), |pf| format!("{pf:+.4}"))),
                _ => "Measured Q not provided: negative P alone does not establish quadrant 3 or numerical PF.".into(),
            });
            if let Some(fit) = &sweep.fitted_return {
                ui.label(RichText::new(format!("UNFITTED passive baseline: P = {:+.3} W, G = {:+.7} S · equivalent AC return fitted to P: {:.5} A RMS at {:.1}°",
                    fit.baseline_active_power_w, fit.baseline_conductance_s,
                    fit.fitted_current_rms_a, fit.fitted_phase_deg)).color(TEAL));
                ui.small(&fit.interpretation);
            } else {
                ui.small("Observed point is recorded separately. The circuit model has NOT been fitted to it; source/tank and switching details remain unknown.");
            }
        }
        ui.separator();
        let rows = sweep.results.len();
        let focus_row = self
            .selected
            .filter(|&i| self.table_focused != Some((self.plot_generation, i)));
        // Key the table's scroll state to the run so a previous run cannot hide
        // the new column header or the newly selected row.
        let table = egui::ScrollArea::both()
            .id_salt(("sweep_table_scroll", self.plot_generation))
            // Leave room for the outer analysis scrollbar on compact windows.
            .max_width((ui.clip_rect().right() - ui.cursor().left() - 8.0).max(100.0))
            .max_height((ui.available_height() * 0.65).clamp(140.0, 420.0))
            .animated(false)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("sweep_table")
                    .striped(true)
                    .min_col_width(95.0)
                    .show(ui, |ui| {
                        for (column, title) in [
                            "C each (µF)",
                            "Load V rms",
                            "Load W",
                            "Input A rms",
                            "Zin (Ω)",
                            "|Γ|",
                            "G total (mS)",
                            "Balance %",
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            let header = ui.strong(title);
                            #[cfg(test)]
                            if column == 0 {
                                self.probe.table_header = Some(header.rect);
                            }
                            #[cfg(not(test))]
                            let _ = (column, header);
                        }
                        ui.end_row();
                        for i in 0..rows {
                            match &sweep.results[i] {
                                Ok(r) => {
                                    let m = &r.metrics;
                                    let selected = self.selected == Some(i);
                                    let mut clicked = false;
                                    let first_cell = ui.selectable_label(
                                        selected,
                                        RichText::new(format!(
                                            "{}{:0.5}",
                                            if selected { "▶ " } else { "" },
                                            m.capacitance_each_uf
                                        ))
                                        .color(if selected { GOLD } else { Color32::LIGHT_GRAY })
                                        .strong(),
                                    );
                                    clicked |= first_cell.clicked();
                                    if focus_row == Some(i) {
                                        ui.scroll_to_rect(first_cell.rect, None);
                                    }
                                    #[cfg(test)]
                                    if selected {
                                        self.probe.selected_table_cell = Some(first_cell.rect);
                                    }
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            format!("{:.4}", m.load_voltage_rms_v),
                                        )
                                        .clicked();
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            format!("{:.4}", m.load_power_w),
                                        )
                                        .clicked();
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            format!("{:.4}", m.input_current_rms_a),
                                        )
                                        .clicked();
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            format!(
                                                "{:.3} {:+.3}j",
                                                m.input_impedance_real_ohm,
                                                m.input_impedance_imag_ohm
                                            ),
                                        )
                                        .clicked();
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            m.reflection_magnitude
                                                .map_or("—".into(), |g| format!("{g:.5}")),
                                        )
                                        .clicked();
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            RichText::new(format!(
                                                "{:+.4}",
                                                1e3 * m.input_admittance_real_s
                                            ))
                                            .color(
                                                if m.input_admittance_real_s < 0.0 {
                                                    ORANGE
                                                } else {
                                                    TEAL
                                                },
                                            ),
                                        )
                                        .clicked();
                                    clicked |= ui
                                        .selectable_label(
                                            selected,
                                            RichText::new(format!(
                                                "{:.3}{}",
                                                m.energy_balance_percent,
                                                if m.quality_pass { "" } else { "  CHECK" }
                                            ))
                                            .color(
                                                if m.quality_pass {
                                                    Color32::LIGHT_GRAY
                                                } else {
                                                    ORANGE
                                                },
                                            ),
                                        )
                                        .clicked();
                                    if clicked {
                                        self.selected = Some(i);
                                    }
                                }
                                Err(error) => {
                                    ui.label(RichText::new("FAILED").color(ORANGE));
                                    ui.label("Solver issue · hover for details")
                                        .on_hover_text(error);
                                    for _ in 0..6 {
                                        ui.label("—");
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(i) = focus_row {
            self.table_focused = Some((self.plot_generation, i));
        }
        #[cfg(test)]
        {
            self.probe.table_viewport = Some(table.inner_rect);
        }
        #[cfg(not(test))]
        let _ = table;
        ui.separator();
        let rows: Vec<_> = sweep
            .results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .collect();
        let selected = self
            .selected
            .and_then(|i| sweep.results.get(i)?.as_ref().ok());
        if let Some(point) = selected {
            let m = &point.metrics;
            ui.label(
                RichText::new(format!(
                    "SELECTED  C1 {:.5} µF · C2 {:.5} µF   |   Load {:.3} V RMS / {:.3} W   |   Input {:.3} A RMS{}",
                    m.capacitance_each_uf,
                    m.capacitance_2_uf,
                    m.load_voltage_rms_v,
                    m.load_power_w,
                    m.input_current_rms_a,
                    if m.quality_pass { "" } else { "   · CHECK" }
                ))
                .color(if m.quality_pass { GOLD } else { ORANGE })
                .strong(),
            );
            if !m.quality_pass {
                ui.label(
                    RichText::new(format!(
                        "Point not validated · energy balance {:.3}% (limit 1%){}",
                        m.energy_balance_percent,
                        if m.calculation_checks.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", m.calculation_checks.join("; "))
                        }
                    ))
                    .color(ORANGE),
                );
            }
            if let Some(check) = &m.resolution_check {
                ui.label(
                    RichText::new(
                        check
                            .trend
                            .as_deref()
                            .unwrap_or("Resolution check incomplete"),
                    )
                    .color(if m.quality_pass { TEAL } else { ORANGE })
                    .strong(),
                );
                let mut stage = Some(check);
                while let Some(comparison) = stage {
                    let detail = comparison
                        .differences_percent
                        .iter()
                        .map(|d| {
                            format!(
                                "{}: {}",
                                d.metric,
                                d.percent
                                    .map_or("unavailable".into(), |v| format!("{v:.4}%"))
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.label(format!(
                        "{} → {} samples/period · max change {} · balance {:.3}% → {}",
                        comparison.base_steps,
                        comparison.refined_steps,
                        comparison
                            .max_difference_percent
                            .map_or("n/a".into(), |v| format!("{v:.3}%")),
                        comparison.base_balance_percent,
                        comparison
                            .refined_balance_percent
                            .map_or("n/a".into(), |v| format!("{v:.3}%")),
                    ))
                    .on_hover_text(detail);
                    if !m.quality_pass {
                        for issue in &comparison.issues {
                            ui.label(RichText::new(format!("CHECK · {issue}")).color(ORANGE));
                        }
                    }
                    stage = comparison.follow_up.as_deref();
                }
            }
        }
        let height = ((ui.available_height() - 24.0) * 0.5).clamp(110.0, 320.0);
        ui.small(
            "Click a sweep plot to select the nearest solved C1 point · failed points are skipped",
        );
        let mut clicked_x = None;
        ui.columns(2, |cols| {
            let (transform, x) = plot_metric(
                &mut cols[0],
                "Load voltage RMS (V)",
                height,
                &rows,
                selected,
                |m| m.load_voltage_rms_v,
                TEAL,
                self.plot_generation,
            );
            clicked_x = clicked_x.or(x);
            #[cfg(test)]
            {
                self.probe.voltage_plot_y_bounds =
                    Some([transform.bounds().min()[1], transform.bounds().max()[1]]);
                self.probe.voltage_plot_transform = Some(transform);
                self.probe.sweep_plot_transforms[0] = Some(transform);
            }
            #[cfg(not(test))]
            let _ = transform;
            if let Some(observed) = &sweep.config.observed_ac {
                cols[1].label(RichText::new("AC active P: model vs lab (W)").color(ORANGE));
                let response = Plot::new(("observed_ac_active_power", self.plot_generation))
                    .height(height)
                    .legend(Legend::default())
                    .x_axis_formatter(|mark, _| capacitance_tick(mark.value))
                    .label_formatter(|pos| {
                        let mut label = sweep_hover(pos, &rows)?;
                        label.push_str(&format!(
                            "\nReported lab P: {:+.4} W at C1 {:.5} µF",
                            observed.active_power_w, observed.capacitance_each_uf
                        ));
                        Some(label)
                    })
                    .show(&mut cols[1], |plot| {
                        let modeled: Vec<[f64; 2]> = rows
                            .iter()
                            .map(|r| {
                                [
                                    r.metrics.capacitance_each_uf.log10(),
                                    if observed.location == Location::Source {
                                        r.metrics.source_power_w
                                    } else {
                                        r.metrics.fundamental_active_power_w
                                    },
                                ]
                            })
                            .collect();
                        plot.line(
                            Line::new("Circuit prediction", modeled)
                                .color(BLUE)
                                .width(2.0),
                        );
                        plot.points(
                            Points::new(
                                "Reported lab point",
                                vec![[
                                    observed.capacitance_each_uf.log10(),
                                    observed.active_power_w,
                                ]],
                            )
                            .color(ORANGE)
                            .radius(6.0),
                        );
                        plot.hline(egui_plot::HLine::new("P=0", 0.0).color(Color32::GRAY));
                        mark_selected(plot, selected.map(|point| &point.metrics));
                        mark_selected_point(
                            plot,
                            selected.map(|point| {
                                let m = &point.metrics;
                                if observed.location == Location::Source {
                                    m.source_power_w
                                } else {
                                    m.fundamental_active_power_w
                                }
                            }),
                            selected.map(|point| &point.metrics),
                        );
                    });
                clicked_x = clicked_x.or(clicked_plot_x(&response));
                #[cfg(test)]
                {
                    self.probe.sweep_plot_transforms[1] = Some(response.transform);
                }
                cols[1].small("C1 (µF) · logarithmic spacing");
            } else {
                let (transform, x) = plot_metric(
                    &mut cols[1],
                    "Load real power (W)",
                    height,
                    &rows,
                    selected,
                    |m| m.load_power_w,
                    BLUE,
                    self.plot_generation,
                );
                clicked_x = clicked_x.or(x);
                #[cfg(test)]
                {
                    self.probe.sweep_plot_transforms[1] = Some(transform);
                }
                #[cfg(not(test))]
                let _ = transform;
            }
        });
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("AC port conductance G (S)").color(ORANGE));
            let response = Plot::new(("AC port conductance G (S)", self.plot_generation))
                .height(height)
                .legend(Legend::default())
                .x_axis_formatter(|mark, _| capacitance_tick(mark.value))
                .label_formatter(|pos| sweep_hover(pos, &rows))
                .show(&mut cols[0], |plot| {
                    let line = |f: fn(&diode_plug_lab::solver::Metrics) -> f64| -> Vec<[f64; 2]> {
                        rows.iter()
                            .map(|r| [r.metrics.capacitance_each_uf.log10(), f(&r.metrics)])
                            .collect()
                    };
                    plot.line(
                        Line::new("Measured total G", line(|m| m.input_admittance_real_s))
                            .color(ORANGE),
                    );
                    if sweep.effective_circuit.ac_return.is_some() {
                        plot.line(
                            Line::new("Passive plug G", line(|m| m.plug_only_admittance_real_s))
                                .color(TEAL),
                        );
                    }
                    plot.hline(egui_plot::HLine::new("G=0", 0.0).color(Color32::GRAY));
                    mark_selected(plot, selected.map(|point| &point.metrics));
                    mark_selected_point(
                        plot,
                        selected.map(|point| point.metrics.input_admittance_real_s),
                        selected.map(|point| &point.metrics),
                    );
                });
            clicked_x = clicked_x.or(clicked_plot_x(&response));
            #[cfg(test)]
            {
                self.probe.sweep_plot_transforms[2] = Some(response.transform);
            }
            cols[0].small("C1 (µF) · logarithmic spacing");
            if sweep.config.reference_ohms.is_some() {
                let (transform, x) = plot_metric(
                    &mut cols[1],
                    "Fundamental reflection |Γ|",
                    height,
                    &rows,
                    selected,
                    |m| m.reflection_magnitude.unwrap_or(0.0),
                    TEAL,
                    self.plot_generation,
                );
                clicked_x = clicked_x.or(x);
                #[cfg(test)]
                {
                    self.probe.sweep_plot_transforms[3] = Some(transform);
                }
                #[cfg(not(test))]
                let _ = transform;
            } else {
                let (transform, x) = plot_metric(
                    &mut cols[1],
                    "Input current RMS (A)",
                    height,
                    &rows,
                    selected,
                    |m| m.input_current_rms_a,
                    TEAL,
                    self.plot_generation,
                );
                clicked_x = clicked_x.or(x);
                #[cfg(test)]
                {
                    self.probe.sweep_plot_transforms[3] = Some(transform);
                }
                #[cfg(not(test))]
                let _ = transform;
            }
        });
        if let Some(index) = clicked_x.and_then(|x| nearest_solved_index(&sweep.results, x)) {
            self.selected = Some(index);
            ui.ctx().request_repaint();
        }
    }

    fn waveform(&self, ui: &mut egui::Ui) {
        let Some(point) = self.selected_result() else {
            ui.label("Select a successful capacitance point in the table.");
            return;
        };
        let m = &point.metrics;
        ui.heading(format!(
            "C1 = {:.5} µF · C2 = {:.5} µF",
            m.capacitance_each_uf, m.capacitance_2_uf
        ));
        ui.label(format!(
            "Load {:.4} W  ·  Source {:.4} W  ·  Balance error {:.4}%  ·  {} ({} cycles)",
            m.load_power_w,
            m.source_power_w,
            m.energy_balance_percent,
            m.convergence_method,
            m.cycles_integrated
        ));
        let h = ((ui.available_height() - 45.0) * 0.5).clamp(150.0, 320.0);
        Plot::new(self.trace_plot_id("voltage_trace"))
            .height(h)
            .legend(Legend::default())
            .label_formatter(|pos| {
                trace_hover(
                    pos,
                    &point.waveform,
                    &[
                        ("Source", |r| r.source_v),
                        ("Port A–B", |r| r.port_v),
                        ("Load P–N", |r| r.load_v),
                    ],
                    "V",
                )
            })
            .show(ui, |plot| {
                for (name, color, value) in [
                    (
                        "Source V",
                        BLUE,
                        (|r: &diode_plug_lab::solver::Trace| r.source_v)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                    (
                        "Port A–B V",
                        TEAL,
                        (|r: &diode_plug_lab::solver::Trace| r.port_v)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                    (
                        "Load P–N V",
                        ORANGE,
                        (|r: &diode_plug_lab::solver::Trace| r.load_v)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                ] {
                    plot.line(
                        Line::new(
                            name,
                            point
                                .waveform
                                .iter()
                                .map(|r| [r.time_s * 1e3, value(r)])
                                .collect::<Vec<_>>(),
                        )
                        .color(color),
                    );
                }
            });
        Plot::new(self.trace_plot_id("current_trace"))
            .height(h)
            .legend(Legend::default())
            .label_formatter(|pos| {
                let mut traces: Vec<(&str, fn(&diode_plug_lab::solver::Trace) -> f64)> = vec![
                    ("Input", |r| r.input_a),
                    ("Load", |r| r.load_a),
                    ("D1", |r| r.diode_1_a),
                    ("D2", |r| r.diode_2_a),
                ];
                if self
                    .sweep
                    .as_ref()
                    .is_some_and(|s| s.effective_circuit.ac_return.is_some())
                {
                    traces.push(("Passive plug", |r| r.plug_input_a));
                    traces.push(("Powered return", |r| r.return_injection_a));
                }
                trace_hover(pos, &point.waveform, &traces, "A")
            })
            .show(ui, |plot| {
                plot.line(
                    Line::new(
                        "Input A",
                        point
                            .waveform
                            .iter()
                            .map(|r| [r.time_s * 1e3, r.input_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(BLUE),
                );
                plot.line(
                    Line::new(
                        "Load A",
                        point
                            .waveform
                            .iter()
                            .map(|r| [r.time_s * 1e3, r.load_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(TEAL),
                );
                if self
                    .sweep
                    .as_ref()
                    .is_some_and(|s| s.effective_circuit.ac_return.is_some())
                {
                    plot.line(
                        Line::new(
                            "Passive plug A",
                            point
                                .waveform
                                .iter()
                                .map(|r| [r.time_s * 1e3, r.plug_input_a])
                                .collect::<Vec<_>>(),
                        )
                        .color(Color32::LIGHT_GREEN),
                    );
                    plot.line(
                        Line::new(
                            "Powered return injection A",
                            point
                                .waveform
                                .iter()
                                .map(|r| [r.time_s * 1e3, r.return_injection_a])
                                .collect::<Vec<_>>(),
                        )
                        .color(Color32::LIGHT_YELLOW),
                    );
                }
                plot.line(
                    Line::new(
                        "Diode D1 A",
                        point
                            .waveform
                            .iter()
                            .map(|r| [r.time_s * 1e3, r.diode_1_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(ORANGE),
                );
                plot.line(
                    Line::new(
                        "Diode D2 A",
                        point
                            .waveform
                            .iter()
                            .map(|r| [r.time_s * 1e3, r.diode_2_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(Color32::LIGHT_RED),
                );
            });
        ui.small("Horizontal axis: time in settled period (ms). Port current uses passive sign convention.");
    }

    fn diagnostics(&self, ui: &mut egui::Ui) {
        let Some(point) = self.selected_result() else {
            ui.label("Select a successful capacitance point in the sweep table.");
            return;
        };
        let m = &point.metrics;
        ui.heading(format!(
            "Power, energy & switching · C1 = {:.5} µF · C2 = {:.5} µF",
            m.capacitance_each_uf, m.capacitance_2_uf
        ));
        egui::Grid::new("diagnostic_values")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                for (name, value) in [
                    ("Source input", format!("{:.5} W", m.source_power_w)),
                    ("Port input", format!("{:.5} W", m.port_power_w)),
                    ("Load total", format!("{:.5} W", m.load_power_w)),
                    ("Load resistor", format!("{:.5} W", m.load_resistor_power_w)),
                    (
                        "Threshold / battery",
                        format!("{:.5} W", m.load_threshold_power_w),
                    ),
                    ("Diode loss", format!("{:.5} W", m.diode_loss_w)),
                    (
                        "Source Rs loss",
                        format!("{:.5} W", m.source_resistor_loss_w),
                    ),
                    ("Power balance", format!("{:.4}%", m.energy_balance_percent)),
                    (
                        "Fundamental P1",
                        format!("{:.5} W", m.fundamental_active_power_w),
                    ),
                    (
                        "Fundamental Q1",
                        format!("{:.5} var", m.fundamental_reactive_power_var),
                    ),
                    (
                        "Input current THD",
                        format!("{:.3}%", 100.0 * m.input_current_thd),
                    ),
                    ("Input peak", format!("{:.5} A", m.input_peak_a)),
                    (
                        "D1 peak / on",
                        format!(
                            "{:.4} A / {:.1}%",
                            m.diode_1_peak_a,
                            m.diode_1_conduction_fraction * 100.0
                        ),
                    ),
                    (
                        "D2 peak / on",
                        format!(
                            "{:.4} A / {:.1}%",
                            m.diode_2_peak_a,
                            m.diode_2_conduction_fraction * 100.0
                        ),
                    ),
                    (
                        "Plug C energy swing",
                        format!("{:.5} J", m.capacitor_energy_swing_j),
                    ),
                    (
                        "Input energy / cycle",
                        format!("{:.5} J", m.input_energy_per_cycle_j),
                    ),
                ] {
                    ui.strong(name);
                    ui.label(value);
                    if name == "Load resistor"
                        || name == "Power balance"
                        || name == "Input peak"
                        || name == "Input energy / cycle"
                    {
                        ui.end_row();
                    }
                }
            });
        ui.separator();
        ui.label(RichText::new(format!("AC port total: G={:+.7} S, P₁={:+.5} W, Rparallel={} · Passive plug alone: G={:+.7} S",
            m.input_admittance_real_s, m.fundamental_active_power_w,
            m.equivalent_parallel_resistance_ohm.map_or("undefined".into(), |r| format!("{r:.5} Ω")),
            m.plug_only_admittance_real_s)).color(if m.input_admittance_real_s < 0.0 { ORANGE } else { TEAL }));
        ui.label(format!("Negative port power: {:.2}% of cycle; energy returned at port: {:.6} J/cycle; source return: {:.6} J/cycle",
            100.0*m.port_negative_power_fraction, m.port_returned_energy_per_cycle_j, m.source_returned_energy_per_cycle_j));
        ui.label(format!("External AC return path: {:+.6} W absorbed (negative = supplies), {:.6} W supplied; DC bus: {:.6} W supplied",
            m.return_path_absorbed_power_w, m.return_path_supplied_power_w, m.external_bus_supplied_power_w));
        if self
            .sweep
            .as_ref()
            .is_some_and(|s| s.effective_circuit.ac_return.is_some())
        {
            ui.label(RichText::new("In this setup, the powered AC return path can make combined G negative; plug-only G and its energy are reported separately.").color(ORANGE));
        }
        let h = ((ui.available_height() - 75.0) * 0.5).clamp(130.0, 320.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("STORED ENERGY").color(TEAL).strong());
            ui.small("J · plug capacitors and load");
        });
        let energy = [
            ScopeChannel::new("C1", TEAL, |r| r.capacitor_1_energy_j),
            ScopeChannel::new("C2", BLUE, |r| r.capacitor_2_energy_j),
            ScopeChannel::new("Load C", ORANGE, |r| r.load_capacitor_energy_j),
            ScopeChannel::new("Load L", Color32::LIGHT_RED, |r| r.load_inductor_energy_j),
        ];
        let _ = scope_plot(
            ui,
            self.trace_plot_id("stored_energy"),
            h,
            &point.waveform,
            &energy,
            "J",
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new("INSTANTANEOUS POWER").color(TEAL).strong());
            ui.small("W · source, port and load");
        });
        let mut power = vec![
            ScopeChannel::new("Source", TEAL, |r| r.input_power_w),
            ScopeChannel::new("Port", BLUE, |r| r.port_power_w),
            ScopeChannel::new("Load", ORANGE, |r| r.load_power_w),
        ];
        if self
            .sweep
            .as_ref()
            .is_some_and(|s| s.effective_circuit.ac_return.is_some())
        {
            power.push(ScopeChannel::new(
                "Return path",
                Color32::LIGHT_YELLOW,
                |r| r.return_path_power_w,
            ));
        }
        let _ = scope_plot(
            ui,
            self.trace_plot_id("instantaneous_power"),
            h,
            &point.waveform,
            &power,
            "W",
        );
        ui.small("Move over either plot for a recorded-sample cursor · time in ms · negative power means energy return.");
    }

    fn branch_traces(&self, ui: &mut egui::Ui) {
        let Some(point) = self.selected_result() else {
            ui.label("Select a successful sweep point first.");
            return;
        };
        ui.heading(format!(
            "Branch behavior · C1 = {:.5} µF · C2 = {:.5} µF",
            point.metrics.capacitance_each_uf, point.metrics.capacitance_2_uf
        ));
        let h = ((ui.available_height() - 120.0) / 4.0).clamp(95.0, 240.0);
        Plot::new(self.trace_plot_id("node_voltages"))
            .height(h)
            .legend(Legend::default())
            .label_formatter(|pos| {
                trace_hover(
                    pos,
                    &point.waveform,
                    &[
                        ("A", |r| r.port_v),
                        ("P", |r| r.node_p_v),
                        ("N", |r| r.node_n_v),
                    ],
                    "V",
                )
            })
            .show(ui, |plot| {
                for (name, color, f) in [
                    (
                        "A (V)",
                        TEAL,
                        (|w: &diode_plug_lab::solver::Trace| w.port_v)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                    (
                        "P (V)",
                        BLUE,
                        (|w: &diode_plug_lab::solver::Trace| w.node_p_v)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                    (
                        "N (V)",
                        ORANGE,
                        (|w: &diode_plug_lab::solver::Trace| w.node_n_v)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                ] {
                    plot.line(
                        Line::new(
                            name,
                            point
                                .waveform
                                .iter()
                                .map(|w| [w.time_s * 1e3, f(w)])
                                .collect::<Vec<_>>(),
                        )
                        .color(color),
                    );
                }
            });
        Plot::new(self.trace_plot_id("capacitor_currents"))
            .height(h)
            .legend(Legend::default())
            .label_formatter(|pos| {
                trace_hover(
                    pos,
                    &point.waveform,
                    &[
                        ("C1", |r| r.capacitor_1_a),
                        ("C2", |r| r.capacitor_2_a),
                        ("Load C", |r| r.load_capacitor_a),
                    ],
                    "A",
                )
            })
            .show(ui, |plot| {
                for (name, color, f) in [
                    (
                        "C1 (A)",
                        TEAL,
                        (|w: &diode_plug_lab::solver::Trace| w.capacitor_1_a)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                    (
                        "C2 (A)",
                        BLUE,
                        (|w: &diode_plug_lab::solver::Trace| w.capacitor_2_a)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                    (
                        "Load C (A)",
                        ORANGE,
                        (|w: &diode_plug_lab::solver::Trace| w.load_capacitor_a)
                            as fn(&diode_plug_lab::solver::Trace) -> f64,
                    ),
                ] {
                    plot.line(
                        Line::new(
                            name,
                            point
                                .waveform
                                .iter()
                                .map(|w| [w.time_s * 1e3, f(w)])
                                .collect::<Vec<_>>(),
                        )
                        .color(color),
                    );
                }
            });
        Plot::new(self.trace_plot_id("clamp_currents"))
            .height(h)
            .legend(Legend::default())
            .label_formatter(|pos| {
                trace_hover(
                    pos,
                    &point.waveform,
                    &[("D1", |r| r.diode_1_a), ("D2", |r| r.diode_2_a)],
                    "A",
                )
            })
            .show(ui, |plot| {
                plot.line(
                    Line::new(
                        "D1 (A)",
                        point
                            .waveform
                            .iter()
                            .map(|w| [w.time_s * 1e3, w.diode_1_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(TEAL),
                );
                plot.line(
                    Line::new(
                        "D2 (A)",
                        point
                            .waveform
                            .iter()
                            .map(|w| [w.time_s * 1e3, w.diode_2_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(ORANGE),
                );
            });
        Plot::new(self.trace_plot_id("source_coil"))
            .height(h)
            .legend(Legend::default())
            .label_formatter(|pos| {
                let x = trace_hover(
                    pos,
                    &point.waveform,
                    &[("Source/tank coil", |r| r.source_coil_a)],
                    "A",
                )?;
                let position = match pos {
                    HoverPosition::NearDataPoint { position, .. }
                    | HoverPosition::Elsewhere { position } => position.x,
                };
                let index = nearest_trace_index(&point.waveform, position)?;
                Some(format!(
                    "{x}\nCoil energy: {:+.6} J",
                    point.waveform[index].source_coil_energy_j
                ))
            })
            .show(ui, |plot| {
                plot.line(
                    Line::new(
                        "Source/tank coil A",
                        point
                            .waveform
                            .iter()
                            .map(|w| [w.time_s * 1e3, w.source_coil_a])
                            .collect::<Vec<_>>(),
                    )
                    .color(Color32::GREEN),
                );
                plot.line(
                    Line::new(
                        "Coil energy (J)",
                        point
                            .waveform
                            .iter()
                            .map(|w| [w.time_s * 1e3, w.source_coil_energy_j])
                            .collect::<Vec<_>>(),
                    )
                    .color(Color32::LIGHT_GRAY),
                );
            });
        ui.small("Time axis: ms. C1/C2 current directions are A → P / A → N; diode currents are B → P / N → B.");
    }

    fn reference_equations(&mut self, ui: &mut egui::Ui) {
        ui.heading("Resonant tank · fundamental equivalent");
        ui.label("Enter tank components to combine them with the selected plug's operating-point admittance.");
        ui.horizontal(|ui| {
            ui.label("Tank L");
            ui.add(egui::TextEdit::singleline(&mut self.tank_l).desired_width(75.0));
            ui.label("Tank C");
            ui.add(egui::TextEdit::singleline(&mut self.tank_c).desired_width(75.0));
            ui.label("Loss G");
            ui.add(egui::TextEdit::singleline(&mut self.tank_g).desired_width(75.0));
        });
        if let Some(point) = self.selected_result() {
            let l = units::parse(&self.tank_l, Unit::Henry, 1.0);
            let c = units::parse(&self.tank_c, Unit::Farad, 1.0);
            let g = units::parse(&self.tank_g, Unit::Siemens, 1.0);
            match (|| {
                let l = l?;
                let c = c?;
                let g = g?;
                analysis::tank(&point.metrics, self.config.circuit.frequency, l, c, g)
                    .map(|estimate| (estimate, l, c))
            })() {
                Ok((t, l, c)) => {
                    ui.label(format!(
                        "Unloaded f₀: {:.5} Hz   ·   estimated shifted f₀: {:.5} Hz",
                        t.unloaded_frequency_hz, t.shifted_frequency_hz
                    ));
                    ui.label(format!(
                        "G total: {:.6} S   ·   B total at drive: {:.6} S   ·   Q parallel: {}",
                        t.total_conductance_s,
                        t.total_susceptance_at_drive_s,
                        t.parallel_loaded_q
                            .map_or("not defined (G≤0)".into(), |q| format!("{q:.4}"))
                    ));
                    ui.label(format!(
                        "Doc series-equivalent: R_s = {} Ω   ·   Q series = {}",
                        t.doc_series_resistance_ohm
                            .map_or("—".into(), |r| format!("{r:.4}")),
                        t.doc_loaded_q.map_or("—".into(), |q| format!("{q:.4}"))
                    ));
                    if t.negative_damping_risk {
                        ui.label(RichText::new("⚠ Negative total G: the frozen operating point pumps energy (negative damping risk).").color(ORANGE));
                    }
                    let m = &point.metrics;
                    // A conservative charge-per-radian proxy: both plug capacitors in parallel.
                    // The actual nonlinear injected charge depends on clamp conduction.
                    let c_proxy = (m.capacitance_each_uf + m.capacitance_2_uf) * 1e-6;
                    let ratio = c_proxy / c;
                    if let Ok(weak) = analysis::weak_coupling(c_proxy, c, l) {
                        ui.label(format!("Weak-coupling proxy: (C1 + C2) / C_tank = {ratio:.4} · {} (< 0.1 is weak)",
                            if weak { "WEAK" } else { "NOT WEAK" }));
                    }
                    let tank_energy = c * m.input_voltage_fundamental_rms_v.powi(2);
                    if tank_energy > 0.0 {
                        ui.small(format!(
                            "Plug-cap energy swing / ½ C_tank Vpeak² ≈ {:.4} (illustrative only)",
                            m.capacitor_energy_swing_j / tank_energy
                        ));
                    }
                }
                Err(e) => {
                    ui.colored_label(ORANGE, e);
                }
            }
        } else {
            ui.label("Select a successful sweep point for tank estimates.");
        }
        ui.small("Estimates hold plug G/B fixed at the drive frequency. Weak-coupling uses an upper-bound capacitance proxy, not measured injected charge. Energy swing is not the same as net energy extracted per cycle. Neither predicts coupled tank stability.");
        ui.separator();
        ui.heading("Single-pulse capacitor reference");
        ui.horizontal(|ui| {
            ui.label("Precharge voltage");
            ui.add(egui::TextEdit::singleline(&mut self.pulse_v).desired_width(90.0));
            ui.label("Pulse rate per capacitor");
            ui.add(egui::TextEdit::singleline(&mut self.pulse_f).desired_width(90.0));
        });
        if let Some(point) = self.selected_result() {
            let v = units::parse(&self.pulse_v, Unit::Volt, 1.0);
            let f = units::parse(&self.pulse_f, Unit::Hertz, 1.0);
            match v.and_then(|v| {
                f.and_then(|f| analysis::pulse(point.metrics.capacitance_each_uf * 1e-6, v, f))
            }) {
                Ok(p) => {
                    ui.label(format!("Energy per capacitor: {:.6} J   ·   ideal two-cap discharge power: {:.6} W",
                    p.energy_per_capacitor_j, p.two_capacitor_max_power_w));
                }
                Err(e) => {
                    ui.colored_label(ORANGE, e);
                }
            }
        }
        ui.small("Ideal full discharge into a resistor after external precharge. This is a separate reference estimate, not predicted plug output power.");
        ui.separator();
        ui.heading("Tank ringdown probe (transient)");
        ui.horizontal(|ui| {
            ui.label("Start V");
            ui.add(egui::TextEdit::singleline(&mut self.ring_start_v).desired_width(70.0));
            ui.label("Cycles");
            ui.add(egui::TextEdit::singleline(&mut self.ring_cycles).desired_width(60.0));
            ui.checkbox(&mut self.ring_clamps_disabled, "Open-circuit (diodes off)");
        });
        if ui.button("Run ringdown").clicked() {
            let outcome = (|| {
                let l = units::parse(&self.tank_l, Unit::Henry, 1.0)?;
                let c = units::parse(&self.tank_c, Unit::Farad, 1.0)?;
                let g = units::parse(&self.tank_g, Unit::Siemens, 1.0)?;
                let v = units::parse(&self.ring_start_v, Unit::Volt, 1.0)?;
                let cycles = units::parse(&self.ring_cycles, Unit::Unitless, 1.0)? as usize;
                let params = RingdownParams {
                    tank_l_h: l,
                    tank_c_f: c,
                    loss_conductance_s: g,
                    initial_voltage_v: v,
                    ring_cycles: cycles,
                    clamp_enabled: !self.ring_clamps_disabled,
                };
                if self.ring_clamps_disabled {
                    open_circuit_probe(&self.config.circuit, params)
                } else {
                    ringdown(&self.config.circuit, params)
                }
            })();
            match outcome {
                Ok(r) => {
                    self.ring_message = format!(
                        "damped f = {:.3} Hz · α = {:.4} /s · effective G = {:+.3e} S{}",
                        r.damped_frequency_hz,
                        r.decay_constant_per_s,
                        r.net_conductance_s,
                        if r.negative_damping {
                            "  ⚠ NEGATIVE DAMPING"
                        } else {
                            ""
                        }
                    );
                    self.ring_result = Some(r);
                }
                Err(e) => {
                    self.ring_message = e;
                    self.ring_result = None;
                }
            }
        }
        ui.label(&self.ring_message);
        ui.separator();
        ui.heading("Single-pulse discharge reference");
        ui.checkbox(
            &mut self.discharge_single_cap,
            "Document case: C1 into R, N held at 0 V",
        );
        ui.small(if self.discharge_single_cap {
            "Isolated single-capacitor discharge: τ = R·C1 and E = ½C1V². Not connected plug output."
        } else {
            "Both plug capacitors discharge as a series pair with diodes off: τ = R·C1/2 (equal C1, C2)."
        });
        if ui.button("Compute discharge").clicked() {
            self.discharge_result = self.selected_result().and_then(|point| {
                let v = units::parse(&self.pulse_v, Unit::Volt, 1.0).ok()?;
                let c = point.metrics.capacitance_each_uf * 1e-6;
                let r = self.config.circuit.resistance;
                if self.discharge_single_cap {
                    analysis::single_cap_discharge(c, v, r, 5.0 * c * r, 200).ok()
                } else {
                    analysis::discharge(c, v, -v, r, 5.0 * 0.5 * c * r, 200).ok()
                }
            });
            self.discharge_message = if self.discharge_result.is_some() {
                "Discharge computed (ideal isolated reference, not steady-state plug power)".into()
            } else {
                "Select a successful sweep point first".into()
            };
        }
        if let Some(d) = &self.discharge_result {
            ui.label(format!(
                "Initial energy: {:.4} J · final caps: {:.3} V / {:.3} V",
                d.initial_energy_j, d.final_cap_1_v, d.final_cap_2_v
            ));
        }
        ui.label(RichText::new(&self.discharge_message).small());
    }

    fn measurement_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Measured AC-side admittance");
        ui.label("Import a scope CSV: time_s,port_V,input_A (simulator export) or time_s,voltage_V,current_A.");
        ui.label("Current must flow INTO the A–B port. Reverse its sign if the current probe points outward.");
        ui.horizontal(|ui| {
            ui.label("CSV file");
            ui.add(egui::TextEdit::singleline(&mut self.measurement_path).desired_width(410.0));
        });
        ui.horizontal(|ui| {
            ui.label("Drive frequency");
            ui.add(
                egui::TextEdit::singleline(&mut self.measurement_frequency).desired_width(110.0),
            );
            ui.label("Hz or SI prefix");
        });
        ui.checkbox(
            &mut self.invert_measured_current,
            "Invert measured current direction",
        );
        if ui.button("Analyze sampled waveform").clicked() {
            let outcome =
                units::parse(&self.measurement_frequency, Unit::Hertz, 1.0).and_then(|f| {
                    measurement::import_csv(
                        Path::new(self.measurement_path.trim()),
                        f,
                        self.invert_measured_current,
                    )
                });
            match outcome {
                Ok(measured) => {
                    self.measurement_message = format!(
                        "Loaded {} samples across {} period(s)",
                        measured.samples, measured.periods
                    );
                    self.measured = Some(measured);
                }
                Err(error) => {
                    self.measurement_message = error;
                    self.measured = None;
                }
            }
        }
        ui.label(&self.measurement_message);
        if let Some(m) = &self.measured {
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "Measured G = {:+.8} S · B = {:+.8} S · Rparallel = {}",
                    m.conductance_s,
                    m.susceptance_s,
                    m.parallel_resistance_ohm
                        .map_or("undefined".into(), |r| format!("{r:.4} Ω"))
                ))
                .color(if m.conductance_s < 0.0 { ORANGE } else { TEAL }),
            );
            ui.label(format!(
                "Fundamental V1 = {:.5} V RMS · I1 = {:.5} A RMS · P1 = {:+.6} W · Q1 = {:+.6} var",
                m.voltage_fundamental_rms_v,
                m.current_fundamental_rms_a,
                m.fundamental_active_power_w,
                m.fundamental_reactive_power_var
            ));
            ui.label(format!("Total average v·i = {:+.6} W · negative-power time = {:.3}% · returned energy = {:.6} J/cycle",
                m.total_average_power_w, 100.0*m.negative_power_fraction, m.returned_energy_per_cycle_j));
            if let Some(point) = self.selected_result() {
                let selected = &point.metrics;
                ui.label(format!("Selected simulation C1 = C2 = {:.5} µF: G total = {:+.8} S, G plug-only = {:+.8} S",
                    selected.capacitance_each_uf, selected.input_admittance_real_s, selected.plug_only_admittance_real_s));
            }
            if (m.fundamental_active_power_w < 0.0) != (m.total_average_power_w < 0.0) {
                ui.label(RichText::new("P1 and total power have different signs: check harmonic power, probe directions and operating-point assumptions.").color(ORANGE));
            }
            ui.small("P₁ = |V₁|²G, Q₁ = −|V₁|²B. Harmonics are excluded from P₁ but included in average instantaneous v·i.");
        }
    }
}

/// Move among successful sweep indices only; failed points remain visible in the table.
fn adjacent_solved(indices: &[usize], current: Option<usize>, direction: isize) -> Option<usize> {
    let position = indices.iter().position(|i| Some(*i) == current);
    match (position, direction) {
        (None, 1) => indices.first().copied(),
        (Some(p), -1) => p.checked_sub(1).and_then(|p| indices.get(p)).copied(),
        (Some(p), 1) => indices.get(p + 1).copied(),
        _ => None,
    }
}

fn nearest_solved_index(results: &[Result<ResultPoint, String>], log_uf: f64) -> Option<usize> {
    if !log_uf.is_finite() {
        return None;
    }
    results
        .iter()
        .enumerate()
        .filter_map(|(index, result)| {
            let c1 = result.as_ref().ok()?.metrics.capacitance_each_uf;
            (c1.is_finite() && c1 > 0.0).then_some((index, (c1.log10() - log_uf).abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

fn clicked_plot_x<R>(response: &egui_plot::PlotResponse<R>) -> Option<f64> {
    response
        .response
        .clicked()
        .then(|| response.response.interact_pointer_pos())
        .flatten()
        .map(|pos| response.transform.value_from_position(pos).x)
}

fn plot_metric(
    ui: &mut egui::Ui,
    name: &str,
    height: f32,
    rows: &[&ResultPoint],
    selected: Option<&ResultPoint>,
    value: impl Fn(&diode_plug_lab::solver::Metrics) -> f64,
    color: Color32,
    generation: u64,
) -> (egui_plot::PlotTransform, Option<f64>) {
    ui.label(RichText::new(name).color(color));
    let data: Vec<[f64; 2]> = rows
        .iter()
        .map(|r| [r.metrics.capacitance_each_uf.log10(), value(&r.metrics)])
        .collect();
    let response = Plot::new((name, generation))
        .height(height)
        .x_axis_formatter(|mark, _| capacitance_tick(mark.value))
        .label_formatter(|pos| sweep_hover(pos, rows))
        .show(ui, |plot| {
            plot.line(Line::new(name, data).color(color).width(2.0));
            mark_selected(plot, selected.map(|point| &point.metrics));
            mark_selected_point(
                plot,
                selected.map(|point| value(&point.metrics)),
                selected.map(|point| &point.metrics),
            );
        });
    ui.small("C1 (µF) · logarithmic spacing");
    (response.transform, clicked_plot_x(&response))
}

/// The plot coordinate is log10(C1 / µF), but the user reads physical µF ticks.
fn capacitance_tick(log_uf: f64) -> String {
    let uf = 10.0_f64.powf(log_uf);
    if !uf.is_finite() || uf <= 0.0 {
        return String::new();
    }
    if uf < 0.01 {
        return format!("{uf:.2e}");
    }
    let digits = if uf < 1.0 {
        3
    } else if uf < 100.0 {
        2
    } else {
        0
    };
    if digits == 0 {
        return format!("{uf:.0}");
    }
    format!("{uf:.digits$}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn mark_selected(
    plot: &mut egui_plot::PlotUi<'_>,
    selected: Option<&diode_plug_lab::solver::Metrics>,
) {
    if let Some(m) = selected {
        plot.vline(
            VLine::new("", m.capacitance_each_uf.log10())
                .color(GOLD)
                .width(2.0)
                .style(LineStyle::dashed_dense())
                .allow_hover(false),
        );
    }
}

fn mark_selected_point(
    plot: &mut egui_plot::PlotUi<'_>,
    y: Option<f64>,
    selected: Option<&diode_plug_lab::solver::Metrics>,
) {
    if let (Some(y), Some(m)) = (y, selected) {
        if y.is_finite() {
            plot.points(
                Points::new("", vec![[m.capacitance_each_uf.log10(), y]])
                    .color(GOLD)
                    .radius(5.0)
                    .allow_hover(false),
            );
        }
    }
}

fn sweep_hover(pos: &HoverPosition<'_>, rows: &[&ResultPoint]) -> Option<String> {
    let x = match pos {
        HoverPosition::NearDataPoint { position, .. } | HoverPosition::Elsewhere { position } => {
            position.x
        }
    };
    if !x.is_finite() || rows.is_empty() {
        return None;
    }
    let m = &rows
        .iter()
        .min_by(|a, b| {
            (a.metrics.capacitance_each_uf.log10() - x)
                .abs()
                .total_cmp(&(b.metrics.capacitance_each_uf.log10() - x).abs())
        })?
        .metrics;
    Some(format!(
        "Nearest computed point (not interpolated)\nC1 {:.6} µF · C2 {:.6} µF\nLoad {:.4} V RMS · {:.4} W\nSource {:+.4} W · Port P₁ {:+.4} W\nInput {:.4} A RMS · G {:+.6} S · B {:+.6} S\n|Γ| {} · balance {:.3}%{}",
        m.capacitance_each_uf,
        m.capacitance_2_uf,
        m.load_voltage_rms_v,
        m.load_power_w,
        m.source_power_w,
        m.fundamental_active_power_w,
        m.input_current_rms_a,
        m.input_admittance_real_s,
        m.input_admittance_imag_s,
        m.reflection_magnitude
            .map_or("—".into(), |g| format!("{g:.5}")),
        m.energy_balance_percent,
        if m.quality_pass { "" } else { " · CHECK" }
    ))
}

fn trace_hover(
    pos: &HoverPosition<'_>,
    waveform: &[diode_plug_lab::solver::Trace],
    traces: &[(&str, fn(&diode_plug_lab::solver::Trace) -> f64)],
    unit: &str,
) -> Option<String> {
    let x = match pos {
        HoverPosition::NearDataPoint { position, .. } | HoverPosition::Elsewhere { position } => {
            position.x
        }
    };
    let nearest = nearest_trace_index(waveform, x)?;
    let sample = &waveform[nearest];
    let mut result = format!(
        "Nearest recorded sample · t = {:.6} ms",
        sample.time_s * 1e3
    );
    for (name, value) in traces {
        result.push_str(&format!("\n{name}: {:+.6} {unit}", value(sample)));
    }
    Some(result)
}

struct ScopeChannel {
    name: &'static str,
    color: Color32,
    value: fn(&diode_plug_lab::solver::Trace) -> f64,
}

impl ScopeChannel {
    fn new(
        name: &'static str,
        color: Color32,
        value: fn(&diode_plug_lab::solver::Trace) -> f64,
    ) -> Self {
        Self { name, color, value }
    }
}

fn scope_value(value: f64, unit: &str) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let magnitude = value.abs();
    if magnitude > 0.0 && magnitude < 1e-15 {
        return format!("{value:.3e} {unit}");
    }
    let (scale, prefix) = if magnitude == 0.0 {
        (1.0, "")
    } else if magnitude < 1e-12 {
        (1e15, "f")
    } else if magnitude < 1e-9 {
        (1e12, "p")
    } else if magnitude < 1e-6 {
        (1e9, "n")
    } else if magnitude < 1e-3 {
        (1e6, "µ")
    } else if magnitude < 1.0 {
        (1e3, "m")
    } else if magnitude >= 1e6 {
        (1e-6, "M")
    } else if magnitude >= 1e3 {
        (1e-3, "k")
    } else {
        (1.0, "")
    };
    if unit == "W" {
        format!("{:+.3} {prefix}{unit}", value * scale)
    } else {
        format!("{:.3} {prefix}{unit}", value * scale)
    }
}

fn scope_plot(
    ui: &mut egui::Ui,
    id: (&'static str, u64, Option<usize>),
    height: f32,
    waveform: &[diode_plug_lab::solver::Trace],
    channels: &[ScopeChannel],
    unit: &str,
) -> (egui_plot::PlotTransform, Option<usize>) {
    let response = Plot::new(id)
        .height(height)
        .legend(Legend::default())
        .show_crosshair(false)
        .show(ui, |plot| {
            for channel in channels {
                plot.line(
                    Line::new(
                        format!("{} ({unit})", channel.name),
                        waveform
                            .iter()
                            .map(|sample| [sample.time_s * 1e3, (channel.value)(sample)])
                            .collect::<Vec<_>>(),
                    )
                    .color(channel.color)
                    .width(1.8),
                );
            }
            let index = plot
                .response()
                .hovered()
                .then(|| plot.pointer_coordinate())
                .flatten()
                .and_then(|pointer| {
                    plot.transform()
                        .frame()
                        .contains(plot.screen_from_plot(pointer))
                        .then_some(pointer.x)
                })
                .and_then(|x| nearest_trace_index(waveform, x));
            if let Some(sample) = index.and_then(|i| waveform.get(i)) {
                let time = sample.time_s * 1e3;
                plot.vline(
                    VLine::new("", time)
                        .color(Color32::from_rgb(175, 191, 204))
                        .style(LineStyle::dashed_dense())
                        .allow_hover(false),
                );
                for channel in channels {
                    let value = (channel.value)(sample);
                    if value.is_finite() {
                        plot.points(
                            Points::new("", vec![[time, value]])
                                .color(channel.color)
                                .radius(4.5)
                                .allow_hover(false),
                        );
                    }
                }
            }
            index
        });
    let index = response
        .response
        .hovered()
        .then(|| ui.ctx().input(|input| input.pointer.hover_pos()))
        .flatten()
        .filter(|pos| response.transform.frame().contains(*pos))
        .and_then(|pos| {
            nearest_trace_index(waveform, response.transform.value_from_position(pos).x)
        });
    if let Some(sample) = index.and_then(|i| waveform.get(i)) {
        response.response.on_hover_ui_at_pointer(|ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(24, 35, 45))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(85, 113, 126)))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(224.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("MEASUREMENT CURSOR")
                                .size(11.0)
                                .color(TEAL)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.small("SAMPLE");
                        });
                    });
                    ui.label(
                        RichText::new(format!("t = {:.4} ms", sample.time_s * 1e3))
                            .font(FontId::monospace(15.0))
                            .color(Color32::WHITE)
                            .strong(),
                    );
                    ui.separator();
                    for channel in channels {
                        ui.horizontal(|ui| {
                            let (dot, _) =
                                ui.allocate_exact_size(egui::vec2(9.0, 16.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3.5, channel.color);
                            ui.label(channel.name);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(scope_value((channel.value)(sample), unit))
                                            .font(FontId::monospace(13.0))
                                            .color(channel.color),
                                    );
                                },
                            );
                        });
                    }
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("NEAREST RECORDED SAMPLE · NOT INTERPOLATED")
                            .size(10.0)
                            .color(Color32::from_gray(150)),
                    );
                });
        });
    }
    (response.transform, index)
}

fn nearest_trace_index(waveform: &[diode_plug_lab::solver::Trace], x_ms: f64) -> Option<usize> {
    if !x_ms.is_finite() || waveform.is_empty() {
        return None;
    }
    let index = waveform.partition_point(|sample| sample.time_s * 1e3 < x_ms);
    let nearest = if index == 0 {
        0
    } else if index == waveform.len() {
        waveform.len() - 1
    } else if (waveform[index].time_s * 1e3 - x_ms).abs()
        < (waveform[index - 1].time_s * 1e3 - x_ms).abs()
    {
        index
    } else {
        index - 1
    };
    Some(nearest)
}

impl eframe::App for LabApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.render(ui);
    }
}

impl LabApp {
    fn header(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        {
            self.probe.header_bar = Some(ui.max_rect());
        }
        let run_state = if self.running {
            format!("RUNNING  ·  {} / {}", self.progress.0, self.progress.1)
        } else if let Some(sweep) = &self.sweep {
            let solved = sweep.results.iter().filter(|r| r.is_ok()).count();
            let checks = sweep
                .results
                .iter()
                .filter(|r| r.as_ref().is_ok_and(|p| !p.metrics.quality_pass))
                .count();
            if checks > 0 || solved != sweep.results.len() {
                format!(
                    "REVIEW · {checks} CHECK · {} failed",
                    sweep.results.len() - solved
                )
            } else {
                format!("COMPUTED · {solved} points (not lab-validated)")
            }
        } else if self.status_error {
            "CHECK INPUTS".into()
        } else {
            "READY".into()
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("DIODE-PLUG").size(18.0).strong().color(TEAL));
            ui.label(
                RichText::new("/  CIRCUIT LABORATORY")
                    .size(12.0)
                    .color(Color32::LIGHT_GRAY),
            );
            if self.tab != 6 {
                let ctx = ui.ctx().clone();
                self.run_controls(ui, &ctx);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(34, 53, 59))
                    .corner_radius(5)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&run_state)
                                .size(12.0)
                                .color(if self.status_error || run_state.starts_with("REVIEW") {
                                    ORANGE
                                } else {
                                    TEAL
                                })
                                .strong(),
                        );
                    });
                let link = ui.label(
                    RichText::new("Aether Research Institute")
                        .size(11.0)
                        .color(Color32::from_gray(148)),
                );
                #[cfg(test)]
                {
                    self.probe.institute_link = Some(link.rect);
                }
                #[cfg(not(test))]
                let _ = link;
            });
        });
        ui.add_space(3.0);
        let (source, range) = if self.running || self.sweep.is_some() {
            let config = self.sweep.as_ref().map_or(&self.config, |s| &s.config);
            (
                format!(
                    "{:.3} V RMS   ·   {:.3} Hz",
                    config.circuit.voltage_rms, config.circuit.frequency
                ),
                format!(
                    "{:.4}–{:.4} µF   ·   {} points",
                    config.capacitance_min_uf, config.capacitance_max_uf, config.points
                ),
            )
        } else {
            (
                "Configure the circuit at left".into(),
                "Run a sweep to analyze C1".into(),
            )
        };
        let selected = self.selected_result().map_or_else(
            || {
                if self.running {
                    "Calculating…".into()
                } else {
                    "Select a solved point".into()
                }
            },
            |point| {
                let m = &point.metrics;
                format!(
                    "C1 {:.5} µF   ·   {:.3} V   ·   {:.3} W{}",
                    m.capacitance_each_uf,
                    m.load_voltage_rms_v,
                    m.load_power_w,
                    if m.quality_pass { "" } else { " · CHECK" }
                )
            },
        );
        let cards = [
            ("ANALYZED SOURCE", source),
            ("CAPACITANCE SWEEP", range),
            ("SELECTED LOAD", selected),
        ];
        ui.horizontal(|ui| {
            let width =
                ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x - 3.0) / 3.0).max(40.0);
            for (index, (label, value)) in cards.iter().enumerate() {
                let card = ui.allocate_ui_with_layout(
                    egui::vec2(width, 42.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::Frame::new()
                            .fill(if index == 2 {
                                Color32::from_rgb(39, 52, 55)
                            } else {
                                Color32::from_rgb(32, 42, 52)
                            })
                            .corner_radius(5)
                            .inner_margin(egui::Margin::symmetric(9, 4))
                            .show(ui, |ui| {
                                ui.set_min_width(width - 18.0);
                                ui.label(
                                    RichText::new(*label)
                                        .size(10.0)
                                        .color(Color32::from_gray(155)),
                                );
                                let response = ui
                                    .add_sized(
                                        [width - 18.0, 18.0],
                                        egui::Label::new(
                                            RichText::new(value).size(13.0).strong().color(
                                                if index == 2 {
                                                    GOLD
                                                } else {
                                                    Color32::from_gray(230)
                                                },
                                            ),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(value);
                                #[cfg(test)]
                                if index == 2 {
                                    self.probe.header_summary = Some(response.rect);
                                }
                                #[cfg(not(test))]
                                let _ = response;
                            });
                    },
                );
                #[cfg(test)]
                {
                    self.probe.header_cards[index] = Some(card.response.rect);
                }
                #[cfg(not(test))]
                let _ = card;
            }
        });
    }

    fn tab_button(&mut self, ui: &mut egui::Ui, tab: usize, title: &str) {
        let selected = self.tab == tab;
        let response = ui.add(
            egui::Button::new(RichText::new(title).color(if selected {
                TEAL
            } else {
                Color32::LIGHT_GRAY
            }))
            .frame(false),
        );
        if response.clicked() {
            self.tab = tab;
        }
        #[cfg(test)]
        if tab == 6 {
            self.probe.docs_button = Some(response.rect);
        }
        if selected {
            ui.painter().line_segment(
                [response.rect.left_bottom(), response.rect.right_bottom()],
                egui::Stroke::new(2.0, TEAL),
            );
        }
    }

    /// Render the whole interface (also exercised by headless layout tests).
    pub fn render(&mut self, ui: &mut egui::Ui) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::Progress(done, total) => {
                    self.progress = (done, total);
                    if done == total {
                        self.status = "All points computed · saving results…".into();
                    }
                }
                Message::Finished(result) => {
                    self.running = false;
                    match *result {
                        Ok((sweep, folder, compute_time)) => {
                            self.plot_generation = self.plot_generation.wrapping_add(1);
                            let failed = sweep.results.iter().filter(|r| r.is_err()).count();
                            let quality_checks = sweep
                                .results
                                .iter()
                                .filter(|result| {
                                    result.as_ref().is_ok_and(|p| !p.metrics.quality_pass)
                                })
                                .count();
                            self.performance = Some(RunPerformance {
                                compute_time,
                                points: sweep.results.len(),
                                workers: sweep.workers_used,
                                failed,
                                quality_checks,
                            });
                            self.started_at = None;
                            self.status = format!(
                                "Computed · {quality_checks} CHECK · {failed} failed · not lab-validated"
                            );
                            self.status_error = failed > 0 || quality_checks > 0;
                            self.selected = sweep
                                .observation_comparison
                                .as_ref()
                                .and_then(|comparison| {
                                    sweep.results.iter().position(|r| {
                                        r.as_ref().is_ok_and(|point| {
                                            point.metrics.capacitance_each_uf
                                                == comparison.simulation_capacitance_each_uf
                                        })
                                    })
                                })
                                .or(sweep.recommended_index)
                                .or_else(|| sweep.results.iter().position(Result::is_ok));
                            self.output = Some(folder);
                            self.sweep = Some(sweep);
                        }
                        Err(error) => {
                            self.status = error;
                            self.status_error = true;
                            self.started_at = None;
                            self.performance = None;
                        }
                    }
                }
            }
        }
        egui::Panel::top("application_title")
            .exact_size(98.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 27, 33))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("application_status")
            .exact_size(38.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 25, 31))
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            if self.tab != 6 {
            egui::Panel::left("configuration_sidebar")
                .exact_size(370.0)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(18, 27, 33))
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show(ui, |ui| self.input_panel(ui));
            }
            egui::CentralPanel::default().show(ui, |ui| {
                // The tab bar stays fixed while the analysis scrolls independently.
                ui.horizontal_wrapped(|ui| {
                    self.tab_button(ui, 0, "Sweep");
                    self.tab_button(ui, 1, "Waveforms");
                    self.tab_button(ui, 2, "Energy");
                    self.tab_button(ui, 3, "Tank");
                    self.tab_button(ui, 4, "Branches");
                    self.tab_button(ui, 5, "Measure");
                    self.tab_button(ui, 6, "Docs");
                    if self.tab != 6 {
                        ui.separator();
                        self.point_selector(ui);
                    }
                });
                #[cfg(test)]
                {
                    self.probe.tab_bar = Some(ui.min_rect());
                }
                ui.separator();
                if let Some(sweep) = &self.sweep {
                    let failed = sweep.results.iter().filter(|r| r.is_err()).count();
                    let checks = sweep.results.iter().filter(|r| r.as_ref().is_ok_and(|p| !p.metrics.quality_pass)).count();
                    if checks > 0 || failed > 0 {
                        egui::Frame::new().fill(Color32::from_rgb(91, 54, 34))
                            .corner_radius(4).inner_margin(egui::Margin::symmetric(9, 4))
                            .show(ui, |ui| {
                                ui.label(RichText::new(format!("⚠ REVIEW REQUIRED · {checks} CHECK · {failed} failed · sweep is not validated"))
                                    .color(Color32::WHITE).strong());
                            });
                    }
                }
                let analysis = egui::ScrollArea::vertical()
                    .id_salt((
                        "analysis_workspace",
                        if self.tab == 6 {
                            self.doc_section + 1
                        } else {
                            0
                        },
                        self.document,
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        0 => self.overview(ui),
                        1 => self.waveform(ui),
                        2 => self.diagnostics(ui),
                        3 => self.reference_equations(ui),
                        4 => self.branch_traces(ui),
                        5 => self.measurement_panel(ui),
                        _ => self.documentation(ui),
                    });
                #[cfg(test)]
                {
                    self.probe.analysis_viewport = Some(analysis.inner_rect);
                }
                #[cfg(not(test))]
                let _ = analysis;
            });
        });
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if let Err(error) = parse_batch(&args) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([1000.0, 680.0]),
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        &format!("Diode-Plug Laboratory v{}", env!("CARGO_PKG_VERSION")),
        options,
        Box::new(|cc| Ok(Box::new(LabApp::new(cc)))),
    ) {
        eprintln!("GUI error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    /// Headless egui pass of the real interface (no eframe driver needed).
    struct Harness {
        ctx: egui::Context,
        app: LabApp,
        frame_time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let app = LabApp::new_with_ctx(&ctx);
            Self {
                ctx,
                app,
                frame_time: 0.0,
            }
        }

        fn pass(&mut self, size: [f32; 2], pointer: Option<egui::Pos2>) {
            self.frame_time += 1.0;
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(self.frame_time),
                    events: pointer
                        .map(|p| vec![egui::Event::PointerMoved(p)])
                        .unwrap_or_default(),
                    ..Default::default()
                },
                |ui| self.app.render(ui),
            );
            output.textures_delta.clear(); // no renderer in the headless test
        }

        fn scroll_form_to_end(&mut self, size: [f32; 2]) {
            let scroll = self.app.probe.input_scroll.expect("form scroll area");
            self.wheel_at(size, scroll.center());
        }

        fn wheel_at(&mut self, size: [f32; 2], pointer: egui::Pos2) {
            self.frame_time += 1.0;
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(self.frame_time),
                    events: vec![
                        egui::Event::PointerMoved(pointer),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -10_000.0),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| self.app.render(ui),
            );
            output.textures_delta.clear();
        }

        fn click(&mut self, size: [f32; 2], position: egui::Pos2) {
            self.frame_time += 1.0;
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(self.frame_time),
                    events: vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| self.app.render(ui),
            );
            output.textures_delta.clear();
        }
    }

    fn assert_footer_reachable(size: [f32; 2]) {
        let mut h = Harness::new();
        h.pass(size, None); // warm up persistent ids / styles
        h.pass(size, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let Some(button) = h.app.probe.run_button else {
            panic!("RUN button not rendered at {size:?}");
        };
        let header = h.app.probe.header_bar.expect("application header");
        assert!(
            header.contains_rect(button),
            "button {button:?} outside header {header:?}"
        );
        assert!(
            h.app
                .probe
                .header_cards
                .iter()
                .flatten()
                .all(|card| !card.intersects(button))
        );
        assert!(button.height() <= 30.0, "header action should stay compact");
        assert!(
            screen.contains_rect(button),
            "RUN button {button:?} outside window {screen:?} at {size:?}"
        );
        if let Some(scroll) = h.app.probe.input_scroll {
            assert!(
                header.max.y <= scroll.min.y,
                "header must sit above scrolling inputs"
            );
            assert!(
                !button.intersects(scroll),
                "RUN button {button:?} overlaps input scroll area {scroll:?} at {size:?}"
            );
            assert!(
                screen.contains_rect(scroll),
                "input scroll area {scroll:?} extends outside the window {screen:?}"
            );
        }
        if let Some(status) = h.app.probe.run_status {
            assert!(
                screen.contains_rect(status),
                "status text {status:?} extends outside the window at {size:?}"
            );
        }
        let bar = h.app.probe.status_bar.expect("application status bar");
        let performance = h.app.probe.performance_line.expect("performance line");
        assert!(
            screen.contains_rect(bar) && bar.contains_rect(performance),
            "screen={screen:?} bar={bar:?} performance={performance:?}"
        );
        assert!(bar.contains_rect(h.app.probe.run_status.unwrap()));
        assert!(bar.contains_rect(h.app.probe.version_line.unwrap()));
        assert!(
            (h.app.probe.run_status.unwrap().center().y - performance.center().y).abs() < 8.0,
            "Status and performance must share one row at {size:?}"
        );
        assert!(!bar.intersects(button));
        if let Some(tabbar) = h.app.probe.tab_bar {
            assert!(
                screen.contains_rect(tabbar),
                "tab bar {tabbar:?} clipped at {size:?}"
            );
        }
        h.scroll_form_to_end(size);
        h.pass(size, None);
        assert!(
            h.app
                .probe
                .header_bar
                .unwrap()
                .contains_rect(h.app.probe.run_button.unwrap())
        );
        let scroll = h.app.probe.input_scroll.expect("form scroll area");
        let edit = h.app.probe.last_input_edit.expect("results directory edit");
        assert!(
            scroll.contains_rect(edit),
            "after scrolling, results directory {edit:?} is still outside form viewport {scroll:?} at {size:?}"
        );
        assert!(!h.app.probe.run_button.unwrap().intersects(scroll));
    }

    fn show(label: &str, size: [f32; 2], mut h: Harness) {
        h.pass(size, None);
        h.pass(size, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let probe = h.app.probe;
        let button = probe.run_button.expect("RUN button");
        let scroll = probe.input_scroll.expect("input form");
        let bar = probe.status_bar.expect("application status bar");
        assert!(probe.header_bar.expect("header").contains_rect(button));
        assert!(
            screen.contains_rect(button) && !button.intersects(scroll),
            "{label}: RUN overlaps form or is outside window at {size:?}"
        );
        assert!(
            !bar.intersects(button) && bar.contains_rect(probe.performance_line.unwrap()),
            "{label}: bar={bar:?} button={button:?} performance={:?}",
            probe.performance_line
        );
        assert!(
            screen.contains_rect(probe.run_status.expect("status")),
            "{label}: status clipped at {size:?}"
        );
        assert!(bar.contains_rect(probe.version_line.expect("version label")));
    }

    #[test]
    fn run_button_and_status_stay_inside_and_clear_of_inputs() {
        for size in [
            [1912.0, 969.0],
            [1440.0, 920.0],
            [1280.0, 800.0],
            [1000.0, 680.0],
            [900.0, 600.0],
            [760.0, 520.0],
        ] {
            assert_footer_reachable(size);
        }

        // Dynamic scenarios exercise the actual progress and status regions.
        for size in [[1440.0, 920.0], [1000.0, 680.0], [760.0, 520.0]] {
            let mut h = Harness::new();
            h.app.running = true;
            h.app.progress = (7, 50);
            show("RUNNING ", size, h);

            let mut h = Harness::new();
            h.app.status = "Source voltage: expected a finite number, e.g. `230`, `230V`, `0.23kV` — this is a really long error message that will wrap onto multiple lines in the footer and might overflow the reserved footer height.".into();
            show("LONG ERR", size, h);

            let mut h = Harness::new();
            h.app.output = Some(std::path::PathBuf::from(
                "/home/rene1/dp-calc/results/run-1793171456281-with-a-really-long-sample-id-appended",
            ));
            show("LONG SAVE", size, h);

            let mut h = Harness::new();
            h.app.running = true;
            h.app.progress = (1, 50);
            h.app.output = Some(std::path::PathBuf::from(
                "/home/rene1/dp-calc/results/previous-run-with-a-long-sample-name",
            ));
            show("PROGRESS AND SAVE", size, h);

            let mut h = Harness::new();
            h.app.apply_preset(14);
            show("POWERED RETURN PRESET", size, h);
        }
    }

    #[test]
    fn header_action_runs_validation_after_form_scroll_and_docs_keep_reading_layout() {
        let size = [760.0, 520.0];
        let mut h = Harness::new();
        h.pass(size, None);
        h.pass(size, None);
        let label = h.app.probe.first_field_label.unwrap();
        let edit = h.app.probe.first_field_edit.unwrap();
        let form = h.app.probe.input_scroll.unwrap();
        assert!(form.contains_rect(label) && form.contains_rect(edit));
        assert!(label.left() < edit.left() && (label.center().y - edit.center().y).abs() < 8.0);
        h.scroll_form_to_end(size);
        h.pass(size, None);
        h.app.form[0] = "not-a-number".into();
        let button = h.app.probe.run_button.unwrap();
        h.click(size, button.center());
        assert!(
            h.app.status_error && !h.app.running,
            "header action should validate the form"
        );
        h.app.tab = 6;
        h.app.probe = Probe::default();
        h.pass(size, None);
        assert!(h.app.probe.input_scroll.is_none() && h.app.probe.run_button.is_none());
    }

    #[test]
    fn completed_performance_uses_measured_compute_time() {
        let summary = RunPerformance {
            compute_time: Duration::from_secs(4),
            points: 20,
            workers: 3,
            failed: 2,
            quality_checks: 1,
        };
        assert_eq!(summary.points_per_second(), 5.0);
        assert!(summary.summary().contains("5.00 points/s"));
        let mut h = Harness::new();
        h.app.performance = Some(summary);
        show("COMPLETED", [1000.0, 680.0], h);
    }

    #[test]
    fn selection_skips_failed_points_and_is_shared_between_views() {
        assert_eq!(adjacent_solved(&[0, 2, 4], Some(0), 1), Some(2));
        assert_eq!(adjacent_solved(&[0, 2, 4], Some(2), -1), Some(0));
        assert_eq!(adjacent_solved(&[0, 2, 4], Some(4), 1), None);

        let circuit = Circuit::default();
        let first = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        let mut second = first.clone();
        second.metrics.capacitance_each_uf = 3.0;
        second.metrics.capacitance_2_uf = 3.0;
        let mut h = Harness::new();
        h.app.sweep = Some(Sweep {
            config: default_config(),
            effective_circuit: circuit,
            results: vec![Ok(first), Err("solver failed".into()), Ok(second)],
            recommended_index: None,
            observation_comparison: None,
            fitted_return: None,
            workers_used: 2,
            engine: String::new(),
            recorded_unix_ms: 0,
        });
        h.app.selected = Some(0);
        h.app.tab = 1; // Waveforms, not the sweep table.
        let size = [1000.0, 680.0];
        h.pass(size, None);
        h.pass(size, None);
        let next = h.app.probe.point_next.unwrap();
        h.click(size, next.center());
        assert_eq!(h.app.selected, Some(2));
        h.app.tab = 2; // Same point in Energy.
        h.pass(size, None);
        assert_eq!(
            h.app.selected_result().unwrap().metrics.capacitance_each_uf,
            3.0
        );
        let point = h.app.selected_result().unwrap();
        let near_three = HoverPosition::Elsewhere {
            position: egui_plot::PlotPoint::new(3.0_f64.log10(), 0.0),
        };
        let nearest = sweep_hover(&near_three, &[point]).unwrap();
        assert!(nearest.contains("C1 3.000000 µF"));
        assert!(nearest.contains("not interpolated"));
        let start = HoverPosition::Elsewhere {
            position: egui_plot::PlotPoint::new(point.waveform[0].time_s * 1e3, 0.0),
        };
        let hover = trace_hover(&start, &point.waveform, &[("Port", |r| r.port_v)], "V").unwrap();
        assert!(hover.contains(&format!("Port: {:+.6} V", point.waveform[0].port_v)));
    }

    #[test]
    fn clicking_sweep_plot_selects_nearest_solved_point_in_other_views() {
        let circuit = Circuit::default();
        let mut first = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        first.metrics.capacitance_each_uf = 1.0;
        let mut last = first.clone();
        last.metrics.capacitance_each_uf = 3.0;
        last.metrics.capacitance_2_uf = 3.0;
        let mut h = Harness::new();
        h.app.sweep = Some(Sweep {
            config: default_config(),
            effective_circuit: circuit,
            results: vec![Ok(first), Err("solver failed".into()), Ok(last)],
            recommended_index: None,
            observation_comparison: None,
            fitted_return: None,
            workers_used: 2,
            engine: String::new(),
            recorded_unix_ms: 0,
        });
        assert_eq!(
            nearest_solved_index(&h.app.sweep.as_ref().unwrap().results, 2.0_f64.log10()),
            Some(2)
        );
        assert_eq!(
            nearest_solved_index(&h.app.sweep.as_ref().unwrap().results, f64::NAN),
            None
        );
        h.app.selected = Some(0);
        let size = [1200.0, 1200.0];
        h.pass(size, None);
        h.pass(size, None);
        let transform = h.app.probe.voltage_plot_transform.unwrap();
        let click = transform.position_from_point(&egui_plot::PlotPoint::new(
            2.0_f64.log10(),
            (transform.bounds().min()[1] + transform.bounds().max()[1]) * 0.5,
        ));
        assert!(transform.frame().contains(click));
        h.click(size, click);
        assert_eq!(
            h.app.selected,
            Some(2),
            "plot click should skip failed point"
        );
        h.app.tab = 1;
        h.pass(size, None);
        assert_eq!(
            h.app.selected_result().unwrap().metrics.capacitance_each_uf,
            3.0
        );
    }

    #[test]
    fn new_sweep_uses_new_plot_bounds_and_physical_capacitance_ticks() {
        assert_eq!(capacitance_tick(-1.0), "0.1");
        assert_eq!(capacitance_tick(0.0), "1");
        assert_eq!(capacitance_tick(1.0), "10");
        assert_eq!(capacitance_tick(2.0), "100");

        let circuit = Circuit::default();
        let mut first = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        first.metrics.load_voltage_rms_v = 12.0;
        let mut second = first.clone();
        second.metrics.capacitance_each_uf = 3.0;
        second.metrics.capacitance_2_uf = 3.0;
        second.metrics.load_voltage_rms_v = 12.5;
        let mut h = Harness::new();
        h.app.sweep = Some(Sweep {
            config: default_config(),
            effective_circuit: circuit,
            results: vec![Ok(first), Ok(second)],
            recommended_index: None,
            observation_comparison: None,
            fitted_return: None,
            workers_used: 2,
            engine: String::new(),
            recorded_unix_ms: 0,
        });
        h.app.selected = Some(0);
        let size = [1000.0, 680.0];
        h.pass(size, None);
        h.pass(size, None);
        let old_bounds = h.app.probe.voltage_plot_y_bounds.unwrap();
        assert!(old_bounds[1] < 20.0, "initial plot bounds: {old_bounds:?}");

        for (i, result) in h.app.sweep.as_mut().unwrap().results.iter_mut().enumerate() {
            result.as_mut().unwrap().metrics.load_voltage_rms_v = 27.0 + i as f64;
        }
        h.app.plot_generation += 1; // New completed run; old plot memory must not be reused.
        h.pass(size, None);
        h.pass(size, None);
        let new_bounds = h.app.probe.voltage_plot_y_bounds.unwrap();
        assert!(
            new_bounds[1] >= 28.0,
            "new run still shows stale bounds: {new_bounds:?}"
        );
        let header = h.app.probe.table_header.unwrap();
        let viewport = h.app.probe.table_viewport.unwrap();
        assert!(
            viewport.contains_rect(header),
            "table header is clipped after new run: {header:?} {viewport:?}"
        );
    }

    #[test]
    fn new_run_resets_prior_table_scroll_position() {
        let circuit = Circuit::default();
        let point = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        let results = (0..25)
            .map(|i| {
                let mut p = point.clone();
                p.metrics.capacitance_each_uf = (i + 1) as f64;
                Ok(p)
            })
            .collect();
        let mut h = Harness::new();
        h.app.sweep = Some(Sweep {
            config: default_config(),
            effective_circuit: circuit,
            results,
            recommended_index: None,
            observation_comparison: None,
            fitted_return: None,
            workers_used: 2,
            engine: String::new(),
            recorded_unix_ms: 0,
        });
        h.app.selected = Some(0);
        let size = [1000.0, 680.0];
        h.pass(size, None);
        h.pass(size, None);
        let viewport = h.app.probe.table_viewport.unwrap();
        h.wheel_at(size, viewport.center());
        h.pass(size, None);
        let old_header = h.app.probe.table_header.unwrap();
        assert!(
            old_header.bottom() < viewport.top(),
            "wheel didn't scroll table: {old_header:?} {viewport:?}"
        );
        h.app.plot_generation += 1;
        h.pass(size, None);
        h.pass(size, None);
        let header = h.app.probe.table_header.unwrap();
        assert!(h.app.probe.table_viewport.unwrap().contains_rect(header));
    }

    #[test]
    fn selection_scrolls_table_once_but_preserves_manual_scroll() {
        let circuit = Circuit::default();
        let point = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        let results = (0..30)
            .map(|i| {
                let mut p = point.clone();
                p.metrics.capacitance_each_uf = (i + 1) as f64;
                Ok(p)
            })
            .collect();
        let mut h = Harness::new();
        h.app.sweep = Some(Sweep {
            config: default_config(),
            effective_circuit: circuit,
            results,
            recommended_index: None,
            observation_comparison: None,
            fitted_return: None,
            workers_used: 2,
            engine: String::new(),
            recorded_unix_ms: 0,
        });
        let size = [1000.0, 680.0];
        h.app.selected = Some(0);
        h.pass(size, None);
        h.pass(size, None);
        h.app.selected = Some(28); // Selection from another tab or a plot.
        h.pass(size, None);
        h.pass(size, None);
        let cell = h.app.probe.selected_table_cell.unwrap();
        let viewport = h.app.probe.table_viewport.unwrap();
        assert!(
            viewport.contains_rect(cell),
            "selected row is offscreen: {cell:?} {viewport:?}"
        );

        h.app.selected = Some(0);
        h.pass(size, None);
        h.pass(size, None);
        let cell = h.app.probe.selected_table_cell.unwrap();
        assert!(h.app.probe.table_viewport.unwrap().contains_rect(cell));
        h.wheel_at(size, h.app.probe.table_viewport.unwrap().center());
        h.pass(size, None);
        assert!(
            h.app.probe.selected_table_cell.unwrap().bottom()
                < h.app.probe.table_viewport.unwrap().top(),
            "table forcibly scrolled back after manual wheel"
        );
    }

    #[test]
    fn every_sweep_plot_selects_a_solved_point_with_or_without_lab_observation() {
        let circuit = Circuit::default();
        let first = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        let mut last = first.clone();
        last.metrics.capacitance_each_uf = 3.0;
        last.metrics.capacitance_2_uf = 3.0;
        let size = [1200.0, 1800.0];
        for observed in [false, true] {
            for reference in [false, true] {
                let mut config = default_config();
                if !reference {
                    config.reference_ohms = None;
                }
                if observed {
                    config.observed_ac = Some(ObservedAc {
                        location: Location::Source,
                        voltage_rms_v: 24.0,
                        capacitance_each_uf: 2.0,
                        active_power_w: -1.0,
                        reactive_power_var: None,
                        note: String::new(),
                    });
                }
                let mut h = Harness::new();
                h.app.sweep = Some(Sweep {
                    config,
                    effective_circuit: circuit.clone(),
                    results: vec![Ok(first.clone()), Err("failed".into()), Ok(last.clone())],
                    recommended_index: None,
                    observation_comparison: None,
                    fitted_return: None,
                    workers_used: 2,
                    engine: String::new(),
                    recorded_unix_ms: 0,
                });
                for plot in 0..4 {
                    h.app.selected = Some(0);
                    h.pass(size, None);
                    h.pass(size, None);
                    let transform = h.app.probe.sweep_plot_transforms[plot].unwrap();
                    let click = transform.position_from_point(&egui_plot::PlotPoint::new(
                        2.0_f64.log10(),
                        (transform.bounds().min()[1] + transform.bounds().max()[1]) * 0.5,
                    ));
                    assert!(
                        h.app.probe.analysis_viewport.unwrap().contains(click),
                        "plot {plot} not visible in test viewport"
                    );
                    h.click(size, click);
                    assert_eq!(
                        h.app.selected,
                        Some(2),
                        "plot {plot} observed={observed} reference={reference}"
                    );
                }
            }
        }
    }

    #[test]
    fn populated_sweep_header_and_controls_fit_compact_windows() {
        let circuit = Circuit::default();
        let point = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        for size in [[1000.0, 680.0], [900.0, 600.0], [760.0, 520.0]] {
            let mut h = Harness::new();
            h.app.sweep = Some(Sweep {
                config: default_config(),
                effective_circuit: circuit.clone(),
                results: vec![Ok(point.clone()); 12],
                recommended_index: None,
                observation_comparison: None,
                fitted_return: None,
                workers_used: 2,
                engine: String::new(),
                recorded_unix_ms: 0,
            });
            h.app.selected = Some(0);
            h.pass(size, None);
            h.pass(size, None);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            for (name, rect) in [
                ("header summary", h.app.probe.header_summary.unwrap()),
                ("point selector", h.app.probe.point_next.unwrap()),
                ("table viewport", h.app.probe.table_viewport.unwrap()),
                ("run button", h.app.probe.run_button.unwrap()),
            ] {
                assert!(
                    screen.contains_rect(rect),
                    "{name} clipped at {size:?}: {rect:?}; analysis={:?}",
                    h.app.probe.analysis_viewport
                );
            }
            let header = h.app.probe.header_bar.unwrap();
            assert!(header.contains_rect(h.app.probe.institute_link.unwrap()));
            let cards = h.app.probe.header_cards.map(|card| card.unwrap());
            for card in cards {
                assert!(
                    header.contains_rect(card),
                    "header card clipped at {size:?}: {card:?} {header:?}"
                );
            }
            assert!(
                cards[0].right() <= cards[1].left() && cards[1].right() <= cards[2].left(),
                "header cards overlap at {size:?}: {cards:?}"
            );
            assert!(
                h.app
                    .probe
                    .analysis_viewport
                    .unwrap()
                    .contains_rect(h.app.probe.table_viewport.unwrap()),
                "table={:?} analysis={:?} size={size:?}",
                h.app.probe.table_viewport,
                h.app.probe.analysis_viewport
            );
        }
    }

    #[test]
    fn energy_scope_cursor_uses_recorded_samples_and_readable_units() {
        assert_eq!(scope_value(0.0, "J"), "0.000 J");
        assert_eq!(scope_value(0.000_012_3, "J"), "12.300 µJ");
        assert_eq!(scope_value(0.000_000_000_2, "J"), "200.000 pJ");
        assert_eq!(scope_value(0.02, "J"), "20.000 mJ");
        assert_eq!(scope_value(-0.5, "W"), "-500.000 mW");
        let point = diode_plug_lab::solver::simulate(&Circuit::default(), 1.0, None).unwrap();
        let channels = [ScopeChannel::new("C1", TEAL, |r| r.capacitor_1_energy_j)];
        let ctx = egui::Context::default();
        let size = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 450.0));
        let mut plot = None;
        let mut first = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(size),
                ..Default::default()
            },
            |ui| {
                plot = Some(scope_plot(
                    ui,
                    ("test_energy_scope", 0, Some(0)),
                    280.0,
                    &point.waveform,
                    &channels,
                    "J",
                ));
            },
        );
        first.textures_delta.clear();
        // Plot axes settle their label widths on the next frame.
        let mut warmup = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(size),
                time: Some(1.0),
                ..Default::default()
            },
            |ui| {
                plot = Some(scope_plot(
                    ui,
                    ("test_energy_scope", 0, Some(0)),
                    280.0,
                    &point.waveform,
                    &channels,
                    "J",
                ));
            },
        );
        warmup.textures_delta.clear();
        let transform = plot.unwrap().0;
        let sample_index = point.waveform.len() / 3;
        let sample = &point.waveform[sample_index];
        let pointer = transform.position_from_point(&egui_plot::PlotPoint::new(
            sample.time_s * 1e3,
            (transform.bounds().min()[1] + transform.bounds().max()[1]) * 0.5,
        ));
        assert!(transform.frame().contains(pointer));
        let mut hovered = None;
        let mut second = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(size),
                time: Some(2.0),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            },
            |ui| {
                hovered = Some(scope_plot(
                    ui,
                    ("test_energy_scope", 0, Some(0)),
                    280.0,
                    &point.waveform,
                    &channels,
                    "J",
                ));
            },
        );
        second.textures_delta.clear();
        let (second_transform, nearest) = hovered.unwrap();
        assert_eq!(
            nearest,
            Some(sample_index),
            "first={:?} second={:?} pointer={pointer:?} first_x={} second_x={}",
            transform.bounds(),
            second_transform.bounds(),
            transform.value_from_position(pointer).x,
            second_transform.value_from_position(pointer).x
        );
    }

    #[test]
    fn reset_restores_editable_inputs_without_erasing_results() {
        let mut h = Harness::new();
        h.app.apply_preset(14);
        h.app.form[0] = "bad input".into();
        h.app.config.operator = "Ada".into();
        h.app.config.run_id = "sample-42".into();
        h.app.parent = "custom-output".into();
        h.app.tank_l = "5mH".into();
        h.app.measurement_path = "old-wave.csv".into();
        let circuit = Circuit::default();
        let point = diode_plug_lab::solver::simulate(&circuit, 1.0, None).unwrap();
        h.app.sweep = Some(Sweep {
            config: default_config(),
            effective_circuit: circuit,
            results: vec![Ok(point)],
            recommended_index: None,
            observation_comparison: None,
            fitted_return: None,
            workers_used: 1,
            engine: String::new(),
            recorded_unix_ms: 0,
        });
        h.app.selected = Some(0);
        let size = [1000.0, 680.0];
        h.pass(size, None);
        h.pass(size, None);
        h.click(size, h.app.probe.reset_button.unwrap().center());
        assert!(h.app.reset_pending);
        h.pass(size, None);
        h.click(size, h.app.probe.reset_confirm.unwrap().center());
        let config = h.app.read_config().unwrap();
        assert_eq!(config.circuit.voltage_rms, 24.0);
        assert_eq!(config.circuit.frequency, 10_000.0);
        assert_eq!(config.points, 50);
        assert_eq!(config.workers, 0);
        assert_eq!(config.reference_ohms, Some(10.0));
        assert_eq!(h.app.parent, "results");
        assert_eq!(h.app.tank_l, "1mH");
        assert!(h.app.measurement_path.is_empty());
        assert_eq!(config.preset_name, None);
        assert!(config.operator.is_empty() && config.run_id.is_empty());
        assert!(!h.app.observed_enabled && !h.app.cap_advanced && !h.app.return_enabled);
        assert!(!h.app.reset_pending);
        assert!(h.app.selected_result().is_some());
    }

    #[test]
    fn technical_documents_are_embedded_navigable_and_render_on_compact_windows() {
        let theory = &DOCUMENT_SECTIONS[0];
        let coverage = &DOCUMENT_SECTIONS[1];
        assert!(
            DOCUMENTS[0].1.contains('‑'),
            "Original Markdown should remain unchanged"
        );
        assert!(theory.iter().chain(coverage).all(|section| {
            !section.title.contains('‑') && !section.markdown.contains('‑')
        }));
        assert!(
            theory
                .iter()
                .any(|section| section.title.contains("Diode-Plug"))
        );
        assert!(
            theory
                .iter()
                .any(|section| section.markdown.contains("diode-plug"))
        );
        assert!(theory.len() > 10);
        assert!(coverage.len() >= 3);
        assert!(theory.iter().any(|section| section.markdown.contains("$$")));
        assert!(
            theory
                .iter()
                .any(|section| section.markdown.contains(DIODE_FIGURE_MARKER))
        );
        let figure = egui_extras::image::load_svg_bytes(DIODE_FIGURE, &Default::default())
            .expect("The bundled circuit diagram must rasterize offline");
        assert!(figure.pixels.iter().filter(|pixel| pixel.a() > 0).count() > 1000);
        assert!(
            String::from_utf8_lossy(&equation_svg(r"\frac{1}{2} C_1 v_{C_1}^2", true).unwrap())
                .contains("<svg")
        );
        assert!(
            theory
                .iter()
                .any(|section| section.markdown.contains("Figure asset is not included"))
        );
        assert!(
            theory
                .iter()
                .all(|section| !section.markdown.contains("!["))
        );
        for size in [[1000.0, 680.0], [760.0, 520.0], [1900.0, 970.0]] {
            let mut h = Harness::new();
            h.pass(size, None);
            h.click(size, h.app.probe.docs_button.unwrap().center());
            assert_eq!(h.app.tab, 6);
            for (document, section) in [(0, 0), (0, 1), (0, 2), (1, 0), (1, 2)] {
                h.app.document = document;
                h.app.doc_section = section;
                h.pass(size, None);
                let screen =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
                assert!(screen.contains_rect(h.app.probe.docs_button.unwrap()));
                assert!(
                    h.app.probe.analysis_viewport.unwrap().left() < 40.0,
                    "Docs should use the full workspace, without the configuration sidebar"
                );
                assert!(h.app.probe.doc_article.unwrap().width() > 500.0);
                if size[0] > 1500.0 {
                    let article = h.app.probe.doc_article.unwrap();
                    assert!(
                        article.left() > 250.0 && article.width() < 1250.0,
                        "Article should form a readable centered column: {article:?}"
                    );
                }
            }
            h.app.document = 0;
            h.app.doc_section = theory
                .iter()
                .position(|s| s.title == "Circuit Topology")
                .unwrap();
            h.pass(size, None);
            assert!(h.app.probe.doc_figure.unwrap().width() > 300.0);
            h.app.tab = 0;
            h.pass(size, None);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            assert!(screen.contains_rect(h.app.probe.run_button.unwrap()));
            assert!(h.app.probe.analysis_viewport.unwrap().left() > 350.0);
        }
    }

    #[test]
    fn every_document_equation_typesets_offline() {
        let mut count = 0;
        for section in DOCUMENT_SECTIONS
            .iter()
            .flat_map(|sections| sections.iter())
        {
            let parser = pulldown_cmark::Parser::new_ext(
                &section.markdown,
                pulldown_cmark::Options::ENABLE_MATH,
            );
            for event in parser {
                let (tex, display) = match event {
                    pulldown_cmark::Event::InlineMath(tex) => (tex, false),
                    pulldown_cmark::Event::DisplayMath(tex) => (tex, true),
                    _ => continue,
                };
                count += 1;
                assert!(
                    equation_svg(&tex, display).is_ok(),
                    "{}: {tex}",
                    section.title
                );
            }
        }
        assert_eq!(
            count,
            DOCUMENTS
                .iter()
                .map(|(_, source)| source.matches(r"\(").count())
                .sum::<usize>(),
            "source equations must all reach the math renderer"
        );
        let diode = DOCUMENT_SECTIONS[0]
            .iter()
            .find(|s| s.title == "Diode Conduction Conditions")
            .unwrap();
        let events: Vec<_> =
            pulldown_cmark::Parser::new_ext(&diode.markdown, pulldown_cmark::Options::ENABLE_MATH)
                .collect();
        assert!(events.iter().any(|event| matches!(event,
            pulldown_cmark::Event::InlineMath(tex) if tex.as_ref() == "v_P(t) < -V_f")));
        assert!(events.iter().any(|event| matches!(event,
            pulldown_cmark::Event::DisplayMath(tex) if tex.contains("i_{D_1}(t)="))));
        assert!(events.iter().all(|event| !matches!(event,
            pulldown_cmark::Event::Text(text) if text.contains(r"\(") || text.contains(r"\)"))));
    }

    #[test]
    fn nested_display_equation_is_not_shrunk_to_the_remaining_line_width() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        for frame in 0..3 {
            let mut rect = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 400.0),
                    )),
                    time: Some(frame as f64),
                    ..Default::default()
                },
                |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(680.0);
                        rect = Some(
                            render_equation_image(
                                ui,
                                r"i_{D_1}(t)=\max\!\left(0,\frac{-v_P(t)-V_f}{R_d}\right)",
                                false,
                            )
                            .rect,
                        );
                    });
                },
            );
            output.textures_delta.clear();
            if frame == 2 {
                let rect = rect.unwrap();
                assert!(
                    rect.width() >= 200.0 && rect.height() >= 30.0,
                    "equation shrunk: {rect:?}"
                );
            }
        }
    }
}
