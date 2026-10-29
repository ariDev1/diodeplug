use crate::solver::Metrics;
use serde::Serialize;
use std::f64::consts::PI;

#[derive(Debug, Serialize)]
pub struct TankEstimate {
    pub unloaded_frequency_hz: f64,
    pub shifted_frequency_hz: f64,
    pub total_conductance_s: f64,
    pub total_susceptance_at_drive_s: f64,
    pub parallel_loaded_q: Option<f64>,
    /// Series-equivalent loss resistance of the tank (doc formula, measured admittance frozen).
    pub doc_series_resistance_ohm: Option<f64>,
    /// ω·L / R_series (doc "loaded Q" via series equivalent).
    pub doc_loaded_q: Option<f64>,
    /// Negative damping risk: the plug/tank composite pumps energy into the mains.
    pub negative_damping_risk: bool,
}

/// Fundamental-frequency parallel-tank estimate, freezing plug admittance at its measured drive point.
pub fn tank(
    metrics: &Metrics,
    drive_hz: f64,
    inductance_h: f64,
    capacitance_f: f64,
    loss_conductance_s: f64,
) -> Result<TankEstimate, String> {
    if ![drive_hz, inductance_h, capacitance_f, loss_conductance_s]
        .iter()
        .all(|x| x.is_finite())
        || drive_hz <= 0.0
        || inductance_h <= 0.0
        || capacitance_f <= 0.0
    {
        return Err("Tank needs positive L, C and drive frequency".into());
    }
    if !loss_conductance_s.is_finite() {
        return Err("Tank loss conductance must be finite".into());
    }
    let omega = 2.0 * PI * drive_hz;
    let g = loss_conductance_s + metrics.input_admittance_real_s;
    let b = omega * capacitance_f - 1.0 / (omega * inductance_h) + metrics.input_admittance_imag_s;
    let plug_b = metrics.input_admittance_imag_s;
    let shifted_omega = 2.0
        / (inductance_h * ((plug_b * plug_b + 4.0 * capacitance_f / inductance_h).sqrt() + plug_b));
    Ok(TankEstimate {
        unloaded_frequency_hz: 1.0 / (2.0 * PI * (inductance_h * capacitance_f).sqrt()),
        shifted_frequency_hz: shifted_omega / (2.0 * PI),
        total_conductance_s: g,
        total_susceptance_at_drive_s: b,
        parallel_loaded_q: if g > 0.0 {
            Some(shifted_omega * capacitance_f / g)
        } else {
            None
        },
        doc_series_resistance_ohm: {
            // Doc series-equivalent of the measured parallel admittance (G, B):
            // R_s = ω²L²G / (1 + (ωLB)²).
            let denom = 1.0 + (omega * inductance_h * b).powi(2);
            Some(omega * omega * inductance_h * inductance_h * g / denom)
        },
        doc_loaded_q: {
            let denom = 1.0 + (omega * inductance_h * b).powi(2);
            let r_ser = omega * omega * inductance_h * inductance_h * g / denom;
            (r_ser.abs() > 1e-15).then(|| omega * inductance_h / r_ser)
        },
        negative_damping_risk: g < 0.0,
    })
}

#[derive(Debug, Serialize)]
pub struct PulseEstimate {
    pub energy_per_capacitor_j: f64,
    pub two_capacitor_max_power_w: f64,
}

/// Upper-bound ideal RC thought experiment: each cap charges to V, fully discharges f times per second.
pub fn pulse(
    capacitance_each_f: f64,
    initial_v: f64,
    repetition_hz: f64,
) -> Result<PulseEstimate, String> {
    if ![capacitance_each_f, initial_v, repetition_hz]
        .iter()
        .all(|x| x.is_finite())
        || capacitance_each_f <= 0.0
        || initial_v < 0.0
        || repetition_hz < 0.0
    {
        return Err("Pulse needs positive C and nonnegative precharge V and rate".into());
    }
    let energy = 0.5 * capacitance_each_f * initial_v * initial_v;
    Ok(PulseEstimate {
        energy_per_capacitor_j: energy,
        two_capacitor_max_power_w: 2.0 * repetition_hz * energy,
    })
}

/// Capacitive weak-coupling screening proxy: effective injected charge per radian
/// per volt, divided by tank capacitance. This is not a measurement of the
/// nonlinear clamp's actual injected charge or net extracted energy.
pub fn weak_coupling(
    injection_capacitance_f: f64,
    tank_capacitance_f: f64,
    tank_inductance_h: f64,
) -> Result<bool, String> {
    if !injection_capacitance_f.is_finite() || injection_capacitance_f < 0.0 {
        return Err("Injection capacitance must be finite and nonnegative".into());
    }
    if !tank_capacitance_f.is_finite() || tank_capacitance_f <= 0.0 {
        return Err("Tank capacitance must be positive".into());
    }
    if !tank_inductance_h.is_finite() || tank_inductance_h <= 0.0 {
        return Err("Tank inductance must be positive".into());
    }
    let ratio = injection_capacitance_f / tank_capacitance_f;
    Ok(ratio < 0.1)
}

#[derive(Debug, Serialize)]
pub struct DischargeSample {
    pub time_s: f64,
    pub cap_1_v: f64,
    pub cap_2_v: f64,
}

#[derive(Debug, Serialize)]
pub struct DischargeResult {
    pub initial_energy_j: f64,
    pub final_cap_1_v: f64,
    pub final_cap_2_v: f64,
    pub samples: Vec<DischargeSample>,
}

/// Closed-form single-pulse discharge of the two plug capacitors into the load:
/// with both clamp diodes off the series pair C1-C2 discharges through the load,
/// the differential voltage v_L = x − y decays with C_series = C/2 and the
/// common-mode pair stays frozen (ideal caps, no leakage).
pub fn discharge(
    capacitance_each_f: f64,
    cap_1_initial_v: f64,
    cap_2_initial_v: f64,
    resistance_ohm: f64,
    duration_s: f64,
    samples: usize,
) -> Result<DischargeResult, String> {
    if !capacitance_each_f.is_finite() || capacitance_each_f <= 0.0 {
        return Err("Capacitance must be positive".into());
    }
    if !cap_1_initial_v.is_finite() || !cap_2_initial_v.is_finite() {
        return Err("Initial capacitor voltages must be finite".into());
    }
    if !resistance_ohm.is_finite() || resistance_ohm <= 0.0 {
        return Err("Resistance must be finite and positive".into());
    }
    if !duration_s.is_finite() || duration_s <= 0.0 || samples == 0 {
        return Err("Duration must be positive and samples nonzero".into());
    }
    let initial_energy =
        0.5 * capacitance_each_f * (cap_1_initial_v.powi(2) + cap_2_initial_v.powi(2));
    let common = 0.5 * (cap_1_initial_v + cap_2_initial_v);
    let diff0 = cap_1_initial_v - cap_2_initial_v;
    // The two equal capacitors form a series equivalent C/2 across the load.
    let tau = 0.5 * capacitance_each_f * resistance_ohm;
    let mut samples_list = Vec::with_capacity(samples + 1);
    for k in 0..=samples {
        let t = duration_s * k as f64 / samples as f64;
        let decay = (-t / tau).exp();
        samples_list.push(DischargeSample {
            time_s: t,
            cap_1_v: common + 0.5 * diff0 * decay,
            cap_2_v: common - 0.5 * diff0 * decay,
        });
    }
    let last = &samples_list[samples_list.len() - 1];
    Ok(DischargeResult {
        initial_energy_j: initial_energy,
        final_cap_1_v: last.cap_1_v,
        final_cap_2_v: last.cap_2_v,
        samples: samples_list,
    })
}

/// Document's isolated C1→R discharge with N held at 0 V.
/// This is a reference circuit, not the connected two-capacitor plug topology.
pub fn single_cap_discharge(
    capacitance_f: f64,
    initial_v: f64,
    resistance_ohm: f64,
    duration_s: f64,
    samples: usize,
) -> Result<DischargeResult, String> {
    let mut result = discharge(
        capacitance_f,
        initial_v,
        0.0,
        resistance_ohm,
        duration_s,
        samples,
    )?;
    let tau = resistance_ohm * capacitance_f;
    for sample in &mut result.samples {
        sample.cap_1_v = initial_v * (-sample.time_s / tau).exp();
        sample.cap_2_v = 0.0;
    }
    result.final_cap_1_v = result.samples.last().unwrap().cap_1_v;
    result.final_cap_2_v = 0.0;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{Circuit, LoadModel, simulate};
    #[test]
    fn pulse_formula_and_invalid_inputs() {
        let p = pulse(10e-6, 100.0, 50.0).unwrap();
        assert!((p.energy_per_capacitor_j - 0.05).abs() < 1e-12);
        assert!((p.two_capacitor_max_power_w - 5.0).abs() < 1e-12);
        assert!(pulse(-1.0, 100.0, 50.0).is_err());
    }
    #[test]
    fn tank_combines_fundamental_port_admittance() {
        let circuit = Circuit {
            voltage_rms: 24.0,
            frequency: 1000.0,
            resistance: 100.0,
            inductance: 0.0,
            load_capacitance: 0.0,
            source_resistance: 1.0,
            diode_drop: 0.7,
            diode_resistance: 1.0,
            load_model: LoadModel::Linear,
            ac_return: None,
            ..Default::default()
        };
        let m = simulate(&circuit, 10.0, None).unwrap().metrics;
        let estimate = tank(&m, 1000.0, 1e-3, 1e-6, 1e-3).unwrap();
        assert!((estimate.unloaded_frequency_hz - 5032.921210).abs() < 0.01);
        assert!(estimate.parallel_loaded_q.unwrap() > 0.0);
        assert!(estimate.doc_series_resistance_ohm.unwrap() > 0.0);
        assert!(!estimate.negative_damping_risk);
        assert!(tank(&m, 1000.0, 0.0, 1e-6, 1e-3).is_err());
        let pumping = tank(&m, 1000.0, 1e-3, 1e-6, -100.0).unwrap();
        assert!(pumping.negative_damping_risk);
        assert!(pumping.doc_series_resistance_ohm.unwrap() < 0.0);
    }

    #[test]
    fn weak_coupling_ratio_threshold() {
        assert!(!weak_coupling(1e-7, 1e-6, 1e-3).unwrap());
        assert!(weak_coupling(1e-8, 1e-6, 1e-3).unwrap());
        assert!(weak_coupling(-1.0, 1e-6, 1e-3).is_err());
        assert!(weak_coupling(1e-8, 0.0, 1e-3).is_err());
    }

    #[test]
    fn discharge_closed_form_matches_rc_expectation() {
        let r = discharge(10e-6, 100.0, -100.0, 200.0, 1e-3, 11).unwrap();
        assert!((r.initial_energy_j - 0.1).abs() < 1e-9);
        // tau = R·C_series = R·C/2 = 1 ms.
        let last = &r.samples[r.samples.len() - 1];
        assert!((last.cap_1_v.abs() - 100.0 / std::f64::consts::E).abs() < 0.01);
        assert!((last.cap_2_v.abs() - 100.0 / std::f64::consts::E).abs() < 0.01);
        assert!((last.time_s - 1e-3).abs() < 1e-12);
        assert!(discharge(0.0, 10.0, 0.0, 1.0, 1.0, 1).is_err());
        assert!(discharge(10e-6, f64::NAN, 0.0, 0.0, 1.0, 1).is_err());
        assert!(discharge(10e-6, 10.0, -10.0, 0.0, 1.0, 1).is_err());
    }

    #[test]
    fn isolated_single_cap_matches_document_rc_law() {
        let result = single_cap_discharge(10e-6, 100.0, 200.0, 2e-3, 10).unwrap();
        assert!((result.initial_energy_j - 0.05).abs() < 1e-12);
        assert!((result.final_cap_1_v - 100.0 / std::f64::consts::E).abs() < 1e-10);
        assert_eq!(result.final_cap_2_v, 0.0);
    }
}
