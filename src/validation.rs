//! Independent consistency checks against the exported settled waveform.
//! Passing these checks is not calibration or proof that the circuit model is physical.
use crate::solver::{Circuit, ResultPoint};
use serde::Serialize;

/// Comparison of independently settled runs at N and 2N samples per period.
/// An error is recorded as CHECK rather than being silently promoted to a pass.
#[derive(Clone, Debug, Serialize)]
pub struct ResolutionCheck {
    pub base_steps: usize,
    pub refined_steps: usize,
    pub tolerance_percent: f64,
    pub base_balance_percent: f64,
    pub refined_balance_percent: Option<f64>,
    pub differences_percent: Vec<MetricDifference>,
    pub max_difference_percent: Option<f64>,
    pub issues: Vec<String>,
    /// Only populated when the original grid needed another resolution comparison.
    pub follow_up: Option<Box<ResolutionCheck>>,
    /// Accepted waveform grid; absent means the point remains CHECK.
    pub accepted_steps: Option<usize>,
    pub trend: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MetricDifference {
    pub metric: &'static str,
    pub percent: Option<f64>,
}

pub const RESOLUTION_TOLERANCE_PERCENT: f64 = 1.0;

pub fn compare_resolution(
    base: &ResultPoint,
    refined: Result<&ResultPoint, &str>,
) -> ResolutionCheck {
    let mut check = ResolutionCheck {
        base_steps: base.metrics.samples_per_period,
        refined_steps: base.metrics.samples_per_period * 2,
        tolerance_percent: RESOLUTION_TOLERANCE_PERCENT,
        base_balance_percent: base.metrics.energy_balance_percent,
        refined_balance_percent: None,
        differences_percent: Vec::new(),
        max_difference_percent: None,
        issues: Vec::new(),
        follow_up: None,
        accepted_steps: None,
        trend: None,
    };
    let fine = match refined {
        Ok(point) => point,
        Err(error) => {
            check.issues.push(format!("Refined solve failed: {error}"));
            return check;
        }
    };
    let a = &base.metrics;
    let b = &fine.metrics;
    check.refined_steps = b.samples_per_period;
    check.refined_balance_percent = Some(b.energy_balance_percent);
    if b.samples_per_period != 2 * a.samples_per_period {
        check
            .issues
            .push("Refined run did not double sampling resolution".into());
    }
    if b.energy_balance_percent > 1.0 || !b.energy_balance_percent.is_finite() {
        check.issues.push(format!(
            "Refined energy balance exceeds 1% ({:.3}%)",
            b.energy_balance_percent
        ));
    }
    check
        .issues
        .extend(b.calculation_checks.iter().map(|s| format!("Refined {s}")));
    let power_scale = a
        .source_power_w
        .abs()
        .max(b.source_power_w.abs())
        .max(a.port_power_w.abs())
        .max(b.port_power_w.abs())
        .max(a.load_power_w.abs())
        .max(b.load_power_w.abs())
        * 1e-3;
    let current_scale = a
        .input_current_rms_a
        .max(b.input_current_rms_a)
        .max(a.load_current_rms_a)
        .max(b.load_current_rms_a)
        * 1e-3;
    let voltage_scale = a.load_voltage_rms_v.max(b.load_voltage_rms_v) * 1e-3;
    for (name, coarse, fine, floor) in [
        (
            "source power",
            a.source_power_w,
            b.source_power_w,
            power_scale,
        ),
        ("port power", a.port_power_w, b.port_power_w, power_scale),
        ("load power", a.load_power_w, b.load_power_w, power_scale),
        (
            "input current RMS",
            a.input_current_rms_a,
            b.input_current_rms_a,
            current_scale,
        ),
        (
            "load current RMS",
            a.load_current_rms_a,
            b.load_current_rms_a,
            current_scale,
        ),
        (
            "load voltage RMS",
            a.load_voltage_rms_v,
            b.load_voltage_rms_v,
            voltage_scale,
        ),
    ] {
        let percent =
            100.0 * (coarse - fine).abs() / coarse.abs().max(fine.abs()).max(floor).max(1e-9);
        if !percent.is_finite() {
            check
                .issues
                .push(format!("{name} difference is non-finite"));
        } else {
            check.max_difference_percent =
                Some(check.max_difference_percent.unwrap_or(0.0).max(percent));
            if percent > check.tolerance_percent {
                check.issues.push(format!(
                    "{name} changes {percent:.3}% on finer time steps (limit {:.1}%)",
                    check.tolerance_percent
                ));
            }
        }
        check.differences_percent.push(MetricDifference {
            metric: name,
            percent: percent.is_finite().then_some(percent),
        });
    }
    check
}

pub fn check_point(circuit: &Circuit, point: &ResultPoint) -> Vec<String> {
    let m = &point.metrics;
    let w = &point.waveform;
    let mut issues = Vec::new();
    if w.len() < 2 || w.len() != m.samples_per_period + 1 {
        return vec!["Waveform length does not match the reported sampling rate".into()];
    }
    let duration = w.last().unwrap().time_s - w[0].time_s;
    if !duration.is_finite()
        || duration <= 0.0
        || w.windows(2).any(|pair| {
            !pair[0].time_s.is_finite()
                || !pair[1].time_s.is_finite()
                || pair[1].time_s <= pair[0].time_s
        })
    {
        return vec!["Waveform timestamps are not finite and increasing".into()];
    }
    // Trapezoidal integration uses the recorded timestamps, not the solver's
    // accumulator or its stored instantaneous power channels.
    let average = |f: &dyn Fn(&crate::solver::Trace) -> f64| -> f64 {
        w.windows(2)
            .map(|pair| 0.5 * (f(&pair[0]) + f(&pair[1])) * (pair[1].time_s - pair[0].time_s))
            .sum::<f64>()
            / duration
    };
    let checks: [(&str, f64, f64); 7] = [
        (
            "source power",
            m.source_power_w,
            average(&|s| s.source_v * s.input_a),
        ),
        (
            "port power",
            m.port_power_w,
            average(&|s| s.port_v * s.input_a),
        ),
        (
            "load dissipated power",
            m.load_power_w,
            average(&|s| {
                let branch = s.load_a - s.load_capacitor_a;
                circuit.resistance * branch * branch + circuit.load_model.threshold() * branch
            }),
        ),
        (
            "input current RMS",
            m.input_current_rms_a,
            average(&|s| s.input_a.powi(2)).sqrt(),
        ),
        (
            "load voltage RMS",
            m.load_voltage_rms_v,
            average(&|s| s.load_v.powi(2)).sqrt(),
        ),
        (
            "load current RMS",
            m.load_current_rms_a,
            average(&|s| s.load_a.powi(2)).sqrt(),
        ),
        (
            "source resistor loss",
            m.source_resistor_loss_w,
            circuit.source_resistance * average(&|s| s.input_a.powi(2)),
        ),
    ];
    for (label, reported, measured) in checks {
        if !reported.is_finite()
            || !measured.is_finite()
            || (reported - measured).abs() > 1e-7 * reported.abs().max(measured.abs()).max(1.0)
        {
            issues.push(format!(
                "{label} disagrees with recorded voltage/current samples"
            ));
        }
    }
    let period = duration;
    for (label, reported, power) in [
        (
            "source cycle energy",
            m.input_energy_per_cycle_j,
            m.source_power_w,
        ),
        (
            "load cycle energy",
            m.load_energy_per_cycle_j,
            m.load_power_w,
        ),
    ] {
        if !reported.is_finite()
            || (reported - power * period).abs()
                > 1e-7 * reported.abs().max((power * period).abs()).max(1e-9)
        {
            issues.push(format!("{label} disagrees with power × period"));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::simulate;

    #[test]
    fn checks_detect_corrupt_metrics_waveforms_and_timestamps() {
        let circuit = Circuit::default();
        let point = simulate(&circuit, 1.0, None).unwrap();
        assert!(check_point(&circuit, &point).is_empty());
        let mut bad = point.clone();
        bad.metrics.source_power_w += 1000.0;
        assert!(
            check_point(&circuit, &bad)
                .iter()
                .any(|s| s.contains("source power"))
        );
        let mut bad = point.clone();
        for sample in &mut bad.waveform {
            sample.input_a *= 2.0;
        }
        assert!(
            check_point(&circuit, &bad)
                .iter()
                .any(|s| s.contains("input current RMS"))
        );
        let mut bad = point.clone();
        bad.waveform[2].time_s = bad.waveform[1].time_s;
        assert!(
            check_point(&circuit, &bad)
                .iter()
                .any(|s| s.contains("timestamps"))
        );
        let mut bad = point.clone();
        bad.metrics.load_power_w = f64::NAN;
        assert!(!check_point(&circuit, &bad).is_empty());
    }

    #[test]
    fn refinement_detects_metric_drift_and_failed_or_invalid_solve() {
        let circuit = Circuit::default();
        let base = simulate(&circuit, 1.0, None).unwrap();
        let refined = crate::solver::simulate_refined(
            &circuit,
            1.0,
            None,
            base.metrics.samples_per_period * 2,
        )
        .unwrap();
        let clean = compare_resolution(&base, Ok(&refined));
        assert!(clean.issues.is_empty(), "{:?}", clean.issues);
        assert_eq!(clean.differences_percent.len(), 6);
        let mut changed = refined.clone();
        changed.metrics.source_power_w *= 1.05;
        let check = compare_resolution(&base, Ok(&changed));
        assert!(
            check
                .issues
                .iter()
                .any(|issue| issue.contains("source power changes"))
        );
        let mut changed = refined.clone();
        changed.metrics.energy_balance_percent = 3.0;
        assert!(
            compare_resolution(&base, Ok(&changed))
                .issues
                .iter()
                .any(|s| s.contains("Refined energy balance"))
        );
        let mut changed = refined;
        changed.metrics.load_power_w = f64::NAN;
        let check = compare_resolution(&base, Ok(&changed));
        assert!(check.issues.iter().any(|s| s.contains("non-finite")));
        assert!(
            check
                .differences_percent
                .iter()
                .any(|d| d.metric == "load power" && d.percent.is_none())
        );
        let error = compare_resolution(&base, Err("did not converge"));
        assert_eq!(error.refined_steps, 2 * base.metrics.samples_per_period);
        assert!(error.issues[0].contains("did not converge"));
    }
}
