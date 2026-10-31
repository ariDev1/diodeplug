use crate::observed::{Location, ObservedAc};
use crate::solver::{AcReturn, Circuit, LoadModel};
use crate::sweep::Config;

pub struct Preset {
    pub name: &'static str,
    pub note: &'static str,
    pub config: Config,
    /// A nominal nameplate rating, not a constant-power or thermal load model.
    pub rated_load: Option<LoadRating>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadRating {
    pub voltage_rms_v: f64,
    pub power_w: f64,
}

fn lamp_rating(name: &str) -> Option<LoadRating> {
    let (voltage_rms_v, power_w) = match name {
        "240 V / 20 W incandescent" => (240.0, 20.0),
        "230 V / 60 W incandescent" => (230.0, 60.0),
        "120 V / 60 W incandescent" => (120.0, 60.0),
        "12 V / 5 W vehicle bulb" => (12.0, 5.0),
        "24 V / 10 W panel lamp" => (24.0, 10.0),
        _ => return None,
    };
    Some(LoadRating {
        voltage_rms_v,
        power_w,
    })
}

fn base(
    voltage: f64,
    frequency: f64,
    resistance: f64,
    inductance: f64,
    load_capacitance: f64,
    c_min: f64,
    c_max: f64,
) -> Config {
    Config {
        circuit: Circuit {
            voltage_rms: voltage,
            frequency,
            resistance,
            inductance,
            load_capacitance,
            source_resistance: 1.0,
            diode_drop: 0.7,
            diode_resistance: 0.5,
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
        capacitance_min_uf: c_min,
        capacitance_max_uf: c_max,
        points: 16,
        reference_ohms: Some(50.0),
        observed_ac: None,
        fit_to_observed_power: false,
        operator: String::new(),
        run_id: String::new(),
        workers: 0,
    }
}

/// Nominal component examples, not measured device characterization.
pub fn all() -> Vec<Preset> {
    let mut list = Vec::new();
    let mut add = |name, note, config| {
        list.push(Preset {
            name,
            note,
            config,
            rated_load: lamp_rating(name),
        })
    };
    add(
        "240 V / 20 W incandescent",
        "20 W is the rating at 240 V ACROSS the lamp (hot R = 2880 Ω), not a power limit. Higher plug voltage can greatly exceed its rating; fixed-R results are not a real lamp or safety prediction.",
        base(240.0, 50.0, 2880.0, 0.0, 0.0, 0.47, 47.0),
    );
    add(
        "230 V / 60 W incandescent",
        "230 V / 50 Hz hot-filament nominal R ≈ 881.7 Ω; temperature effects omitted.",
        base(230.0, 50.0, 230.0 * 230.0 / 60.0, 0.0, 0.0, 1.0, 100.0),
    );
    add(
        "120 V / 60 W incandescent",
        "120 V / 60 Hz hot-filament nominal R = 240 Ω; no cold-filament inrush.",
        base(120.0, 60.0, 240.0, 0.0, 0.0, 1.0, 100.0),
    );
    add(
        "12 V / 5 W vehicle bulb",
        "12 V nominal hot-filament R = 28.8 Ω; source 12 V AC test fixture.",
        base(12.0, 50.0, 28.8, 0.0, 0.0, 10.0, 1000.0),
    );
    add(
        "24 V / 10 W panel lamp",
        "24 V nominal hot-filament R = 57.6 Ω; source 24 V AC test fixture.",
        base(24.0, 50.0, 57.6, 0.0, 0.0, 10.0, 1000.0),
    );
    let mut heater = base(24.0, 50.0, 24.0, 0.0, 0.0, 10.0, 1000.0);
    heater.circuit.source_resistance = 0.5;
    add(
        "24 V / 24 W heater",
        "24 Ω nominal resistive heating element; no temperature coefficient.",
        heater,
    );
    let mut solenoid = base(24.0, 50.0, 48.0, 0.1, 0.0, 1.0, 330.0);
    solenoid.circuit.source_resistance = 2.0;
    add(
        "24 V / 12 W solenoid coil",
        "48 Ω / 100 mH series-coil approximation; no moving armature or flyback parasitics.",
        solenoid,
    );
    add(
        "8 Ω speaker test coil",
        "8 Ω / 1 mH small-signal electrical proxy; no mechanical back-EMF or acoustic behavior.",
        base(5.0, 1000.0, 8.0, 0.001, 0.0, 0.1, 100.0),
    );
    add(
        "1 kΩ / 10 nF RC fixture",
        "Parallel 10 nF across a 1 kΩ branch on a 5 V / 1 kHz source.",
        base(5.0, 1000.0, 1000.0, 0.0, 10e-9, 0.01, 10.0),
    );
    add(
        "5 µH / 200 mΩ test coil",
        "Series 5 µH / 200 mΩ winding, 1 V RMS / 100 kHz low-impedance fixture.",
        base(1.0, 100_000.0, 0.2, 5e-6, 0.0, 0.001, 1.0),
    );
    let mut battery = base(36.0, 50.0, 0.15, 0.0, 470e-6, 1.0, 470.0);
    battery.circuit.load_model = LoadModel::Battery {
        open_circuit_voltage: 24.0,
    };
    battery.circuit.source_resistance = 2.0;
    add(
        "24 V battery charging proxy",
        "24 V fixed open-circuit EMF + 150 mΩ internal R, ideal reverse-current blocking; 36 V AC test source. No SOC or chemistry.",
        battery,
    );
    let mut battery12 = base(18.0, 50.0, 0.1, 0.0, 220e-6, 1.0, 470.0);
    battery12.circuit.load_model = LoadModel::Battery {
        open_circuit_voltage: 12.0,
    };
    add(
        "12 V battery charging proxy",
        "12 V fixed EMF + 100 mΩ internal R, ideal blocking; 18 V AC test source. No SOC.",
        battery12,
    );
    let mut led = base(36.0, 1000.0, 1.0, 0.0, 220e-6, 0.1, 100.0);
    led.circuit.load_model = LoadModel::Led { knee_voltage: 27.0 };
    led.circuit.source_resistance = 0.5;
    add(
        "30 V / 3000 mA LED array",
        "Nominal 30 V at 3 A: 27 V effective knee + 1 Ω dynamic resistance, ideal reverse blocking. No driver or thermal effects.",
        led,
    );
    let mut led12 = base(18.0, 1000.0, 2.0, 0.0, 100e-6, 0.1, 100.0);
    led12.circuit.load_model = LoadModel::Led { knee_voltage: 10.0 };
    add(
        "12 V / 1 A LED module",
        "10 V knee + 2 Ω dynamic resistance gives 1 A at 12 V, ideal reverse blocking; no constant-current driver.",
        led12,
    );
    let mut powered = base(24.0, 50.0, 100.0, 0.0, 0.0, 1.0, 10.0);
    powered.circuit.source_resistance = 2.0;
    powered.circuit.diode_resistance = 1.0;
    powered.circuit.ac_return = Some(AcReturn {
        current_rms_a: 0.1,
        phase_deg: 0.0,
    });
    add(
        "Powered AC return · negative G example",
        "External switched-return equivalent at A–B: powered 0.1 A RMS in-phase current injection. The passive plug is separate; compare total and plug-only G.",
        powered,
    );
    let mut reported = base(240.0, 50.0, 2.0, 0.0, 0.0, 5.0, 20.0);
    reported.points = 5;
    reported.reference_ohms = None;
    reported.circuit.source_resistance = 0.1;
    reported.circuit.diode_resistance = 1.0;
    reported.observed_ac = Some(ObservedAc {
        location: Location::Source,
        voltage_rms_v: 240.0,
        capacitance_each_uf: 10.0,
        active_power_w: -29.0,
        reactive_power_var: None,
        note: "User-reported 240 VAC / 50 Hz grid, C1=C2=10 µF, load 1–3 Ω, P≈−29 W, negative PF / quadrant 3. Q magnitude, meter convention, waveform, Rs and source/tank details not supplied; Rs=0.1 Ω and R=2 Ω are test assumptions.".into(),
    });
    reported.fit_to_observed_power = true;
    add(
        "240 VAC · 2 Ω · 10 µF · observed −29 W",
        "Lab observation is shown separately from the passive circuit calculation. R=2 Ω and Rs=0.1 Ω are illustrative; no Q value supplied, so quadrant 3 cannot be computed from P alone.",
        reported,
    );
    // ---- EU mains (230 V, 50 Hz) ----
    add(
        "230 V / 10 W LED (EU)",
        "230 V AC, 50 Hz, LED knee ~27 V, 1 Ω dynamic, ideal reverse blocking; no driver.",
        {
            let mut led = base(230.0, 50.0, 1.0, 0.0, 220e-6, 0.1, 10.0);
            led.circuit.load_model = LoadModel::Led { knee_voltage: 27.0 };
            led.circuit.source_resistance = 0.5;
            led
        },
    );
    add(
        "230 V / 2 kW heater (EU)",
        "230 V AC, 50 Hz, purely resistive heating element; nominal R = 230²/2000 ≈ 26.5 Ω.",
        {
            let mut h = base(230.0, 50.0, 230.0 * 230.0 / 2000.0, 0.0, 0.0, 0.1, 1.0);
            h.circuit.source_resistance = 0.1;
            h.circuit.diode_resistance = 0.01;
            h
        },
    );
    // ---- US mains (120 V, 60 Hz) ----
    add(
        "120 V / 100 W incandescent (US)",
        "120 V AC, 60 Hz, hot-filament nominal R = 144 Ω; no cold inrush.",
        base(120.0, 60.0, 144.0, 0.0, 0.0, 1.0, 100.0),
    );
    add(
        "120 V / 15 A outlet (US)",
        "120 V AC, 60 Hz, outlet test fixture: 12 Ω resistance and 100 µH coil approximating typical branch.",
        {
            let mut out = base(120.0, 60.0, 12.0, 100e-6, 0.0, 1.0, 100.0);
            out.circuit.source_resistance = 0.2;
            out
        },
    );
    // ---- Source-side series L + junction capacitance (dynamic clamp node) ----
    {
        let mut doc = base(240.0, 50.0, 576.0, 0.0, 0.0, 0.47, 10.0);
        doc.circuit.source_resistance = 20.0;
        doc.circuit.source_inductance = 0.15;
        doc.circuit.diode_capacitance_f = 1e-9;
        doc.points = 12;
        add(
            "Doc example · 240 V / 50 Hz high-Z source + Ls",
            "Doc reproduction: 240 V / 50 Hz behind Rs = 20 Ω and Ls = 0.15 H into 576 Ω; \
             Cj = 1 nF makes the clamp node dynamic so source inductance takes effect.",
            doc,
        );
    }
    {
        let mut unequal = base(24.0, 50.0, 100.0, 0.0, 0.0, 1.0, 47.0);
        unequal.circuit.capacitor_ratio = 0.5;
        unequal.circuit.capacitor_leakage_parallel_ohm = Some(1.0e6);
        unequal.points = 10;
        add(
            "Unequal caps · C2 = 0.5·C1, 1 MΩ leakage",
            "C1 ≠ C2 asymmetry with parallel leakage on both plug capacitors; the sweep label \
             still states the C1 value, C2 = 0.5·C1.",
            unequal,
        );
    }
    for preset in &mut list {
        preset.config.preset_name = Some(preset.name.to_owned());
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::simulate;
    #[test]
    fn realistic_presets_have_explicit_models_and_valid_inputs() {
        let presets = all();
        assert!(presets.len() >= 10);
        for preset in &presets {
            preset.config.validate().unwrap();
        }
        assert!((presets[0].config.circuit.resistance - 2880.0).abs() < 1e-10);
        assert!(
            presets
                .iter()
                .any(|p| matches!(p.config.circuit.load_model, LoadModel::Battery { .. }))
        );
        assert!(
            presets
                .iter()
                .any(|p| matches!(p.config.circuit.load_model, LoadModel::Led { .. }))
        );
    }
    #[test]
    fn preset_nominal_points_have_finite_balanced_results() {
        for preset in all() {
            if ["EU", "US", "Doc example"]
                .iter()
                .any(|k| preset.name.contains(k))
            {
                continue;
            }
            let c = &preset.config;
            let cap = (c.capacitance_min_uf * c.capacitance_max_uf).sqrt();
            let result = simulate(&c.circuit, cap, c.reference_ohms)
                .unwrap_or_else(|e| panic!("{}: {e}", preset.name));
            assert!(
                result.metrics.energy_balance_percent < 1.0,
                "{}: {:?}",
                preset.name,
                result.metrics
            );
        }
    }
    #[test]
    fn preset_sweep_endpoints_produce_results() {
        for preset in all() {
            if ["EU", "US", "Doc example"]
                .iter()
                .any(|k| preset.name.contains(k))
            {
                continue;
            }
            let c = &preset.config;
            for cap in [c.capacitance_min_uf, c.capacitance_max_uf] {
                let result = simulate(&c.circuit, cap, c.reference_ohms)
                    .unwrap_or_else(|e| panic!("{} at {cap} µF: {e}", preset.name));
                assert!(result.metrics.source_power_w.is_finite());
                assert!(
                    result.metrics.quality_pass,
                    "{} at {cap} µF: balance {:.3}%, checks {:?}",
                    preset.name,
                    result.metrics.energy_balance_percent,
                    result.metrics.calculation_checks
                );
            }
        }
    }
    #[test]
    fn powered_return_preset_separates_external_energy_from_plug() {
        let preset = all()
            .into_iter()
            .find(|p| p.config.circuit.ac_return.is_some())
            .unwrap();
        let m = simulate(&preset.config.circuit, 10.0, preset.config.reference_ohms)
            .unwrap()
            .metrics;
        assert!(m.quality_pass);
        assert!(m.input_admittance_real_s < 0.0);
        assert!(m.plug_only_admittance_real_s > 0.0);
        assert!(m.return_path_supplied_power_w > 0.0);
    }

    #[test]
    fn doc_reproduction_preset_runs_dynamic_source_inductance_mode() {
        let preset = all()
            .into_iter()
            .find(|p| p.name.contains("Doc example"))
            .unwrap();
        let c = &preset.config;
        assert!(c.circuit.source_inductance > 0.0);
        assert!(c.circuit.diode_capacitance_f > 0.0);
        let cap = (c.capacitance_min_uf * c.capacitance_max_uf).sqrt();
        let m = simulate(&c.circuit, cap, c.reference_ohms)
            .unwrap_or_else(|e| panic!("doc preset: {e}"))
            .metrics;
        // Dynamic mode accepts a relaxed quadrature-limited balance.
        assert!(m.energy_balance_percent < 5.0, "{m:?}");
        assert!(m.source_coil_current_rms_a > 0.0);
        assert!(m.source_coil_energy_swing_j > 0.0);
    }

    #[test]
    fn unequal_caps_preset_reports_asymmetric_capacitance() {
        let preset = all()
            .into_iter()
            .find(|p| p.name.contains("Unequal caps"))
            .unwrap();
        let c = &preset.config;
        assert!((c.circuit.capacitor_ratio - 0.5).abs() < 1e-12);
        assert!(c.circuit.capacitor_leakage_parallel_ohm.is_some());
        let cap = (c.capacitance_min_uf * c.capacitance_max_uf).sqrt();
        let m = simulate(&c.circuit, cap, c.reference_ohms)
            .unwrap_or_else(|e| panic!("unequal preset: {e}"))
            .metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
        assert!((m.capacitance_2_uf - cap * 0.5).abs() < 1e-9);
        assert!(m.capacitor_leakage_loss_w > 0.0);
        assert!(m.source_coil_current_rms_a == 0.0);
    }
}
