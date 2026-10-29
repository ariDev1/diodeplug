use crate::observed::{Comparison, Location, ObservedAc};
use crate::solver::{AcReturn, Circuit, Metrics, ResultPoint, simulate, simulate_refined};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

fn solve_checked(
    circuit: &Circuit,
    cap: f64,
    reference: Option<f64>,
) -> Result<ResultPoint, String> {
    let base = simulate(circuit, cap, reference)?;
    Ok(stabilize_point(base, |steps| {
        simulate_refined(circuit, cap, reference, steps)
    }))
}

fn stabilize_point(
    mut base: ResultPoint,
    mut rerun: impl FnMut(usize) -> Result<ResultPoint, String>,
) -> ResultPoint {
    let fine_result = rerun(base.metrics.samples_per_period * 2);
    let mut first =
        crate::validation::compare_resolution(&base, fine_result.as_ref().map_err(String::as_str));
    if base.metrics.quality_pass && first.issues.is_empty() {
        first.accepted_steps = Some(base.metrics.samples_per_period);
        first.trend = Some("Stable on the original and doubled grids".into());
        base.metrics.resolution_check = Some(first);
        return base;
    }
    if let Ok(mut current) = fine_result {
        // Keep doubling only questionable points, up to a fixed safety limit.
        // A grid is accepted only when *it* passes balance/waveform checks and
        // agrees with a separately settled run at the next resolution.
        let mut levels = Vec::new();
        let mut accepted = None;
        let mut stop_reason = None;
        while current.metrics.samples_per_period <= 40_000 {
            let next = rerun(current.metrics.samples_per_period * 2);
            let comparison = crate::validation::compare_resolution(
                &current,
                next.as_ref().map_err(String::as_str),
            );
            let stable = current.metrics.quality_pass && comparison.issues.is_empty();
            levels.push(comparison);
            if stable {
                accepted = Some(current);
                break;
            }
            match next {
                Ok(point) => current = point,
                Err(_) => {
                    stop_reason = Some(
                        "A finer solve failed; cannot establish a stable adjacent pair; CHECK",
                    );
                    break;
                }
            }
        }
        let improving = levels.last().is_some_and(|latest| {
            first
                .max_difference_percent
                .zip(latest.max_difference_percent)
                .is_some_and(|(a, b)| b < a)
                && first
                    .refined_balance_percent
                    .zip(latest.refined_balance_percent)
                    .is_some_and(|(a, b)| b < a)
        });
        first.trend = Some(if let Some(point) = &accepted {
            format!(
                "Stable at {} samples/period; coarser grid discarded",
                point.metrics.samples_per_period
            )
        } else if let Some(reason) = stop_reason {
            reason.into()
        } else if improving {
            "Improving, but the 80,000-step safety limit was reached before verification; CHECK"
                .into()
        } else {
            "Not consistently improving; no verified grid within the 80,000-step limit; CHECK"
                .into()
        });
        for mut level in levels.into_iter().rev() {
            level.follow_up = first.follow_up.take();
            first.follow_up = Some(Box::new(level));
        }
        if let Some(mut point) = accepted {
            first.accepted_steps = Some(point.metrics.samples_per_period);
            point.metrics.resolution_check = Some(first);
            return point;
        }
    } else {
        first.trend =
            Some("Doubled-resolution solve failed; no adjacent finer comparison; CHECK".into());
    }
    base.metrics.quality_pass = false;
    base.metrics.resolution_check = Some(first);
    base
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub circuit: Circuit,
    #[serde(default)]
    pub preset_name: Option<String>,
    pub capacitance_min_uf: f64,
    pub capacitance_max_uf: f64,
    pub points: usize,
    pub reference_ohms: Option<f64>,
    #[serde(default)]
    pub observed_ac: Option<ObservedAc>,
    #[serde(default)]
    pub fit_to_observed_power: bool,
    #[serde(default)]
    pub operator: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub workers: usize,
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        self.circuit.validate()?;
        if !self.capacitance_min_uf.is_finite()
            || !self.capacitance_max_uf.is_finite()
            || self.capacitance_min_uf <= 0.0
            || self.capacitance_max_uf <= self.capacitance_min_uf
        {
            return Err("Capacitor range must have 0 < minimum < maximum".into());
        }
        if !(2..=500).contains(&self.points) {
            return Err("Sweep points must be from 2 to 500".into());
        }
        if self
            .reference_ohms
            .is_some_and(|z| !z.is_finite() || z <= 0.0)
        {
            return Err("Reference impedance must be positive".into());
        }
        if self.workers > 256 {
            return Err("Workers must be 0 (automatic) or at most 256".into());
        }
        if let Some(observed) = &self.observed_ac {
            observed.validate()?;
            if observed.capacitance_each_uf < self.capacitance_min_uf
                || observed.capacitance_each_uf > self.capacitance_max_uf
            {
                return Err("Observed C each must be inside the sweep range".into());
            }
        }
        if self.fit_to_observed_power {
            if self.observed_ac.is_none() {
                return Err("Power fit needs an observed AC operating point".into());
            }
            if self.circuit.ac_return.is_some() {
                return Err(
                    "Power fit cannot be combined with a manually specified AC return".into(),
                );
            }
        }
        Ok(())
    }
    pub fn workers(&self) -> usize {
        let requested = if self.workers == 0 {
            std::thread::available_parallelism().map_or(1, |n| n.get())
        } else {
            self.workers
        };
        requested.min(self.points.max(1))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Sweep {
    pub config: Config,
    pub effective_circuit: Circuit,
    pub results: Vec<Result<ResultPoint, String>>,
    pub recommended_index: Option<usize>,
    pub observation_comparison: Option<Comparison>,
    pub fitted_return: Option<FittedReturn>,
    pub workers_used: usize,
    pub engine: String,
    pub recorded_unix_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
pub struct FittedReturn {
    pub baseline_active_power_w: f64,
    pub target_active_power_w: f64,
    pub baseline_conductance_s: f64,
    pub fitted_current_rms_a: f64,
    pub fitted_phase_deg: f64,
    pub baseline_metrics: Metrics,
    pub interpretation: String,
}

fn active_power(metrics: &Metrics, location: Location) -> f64 {
    match location {
        Location::Source => metrics.source_power_w,
        Location::Port => metrics.fundamental_active_power_w,
    }
}

fn fit_return(config: &Config) -> Result<(AcReturn, FittedReturn), String> {
    let observed = config
        .observed_ac
        .as_ref()
        .ok_or("Observed point required")?;
    let cap = observed.capacitance_each_uf;
    let circuit = &config.circuit;
    let baseline = simulate(circuit, cap, None)
        .map_err(|e| format!("Cannot fit: passive baseline at {cap} µF failed: {e}"))?
        .metrics;
    let baseline_power = active_power(&baseline, observed.location);
    let target = observed.active_power_w;
    let phase = if target <= baseline_power { 0.0 } else { 180.0 };
    let evaluate = |amplitude: f64| -> Result<f64, String> {
        let mut trial = circuit.clone();
        trial.ac_return = Some(AcReturn {
            current_rms_a: amplitude,
            phase_deg: phase,
        });
        let m = simulate(&trial, cap, None)
            .map_err(|e| format!("Cannot fit AC return at {amplitude:.6} A: {e}"))?
            .metrics;
        if !m.quality_pass {
            return Err(format!(
                "Fit trial failed numerical power balance at {amplitude:.6} A"
            ));
        }
        Ok(active_power(&m, observed.location))
    };
    let mut low = 0.0;
    let mut high = ((baseline_power - target).abs() / circuit.voltage_rms).max(1e-4) * 1.5;
    let decreasing = phase == 0.0;
    for _ in 0..12 {
        let p = evaluate(high)?;
        if (decreasing && p <= target) || (!decreasing && p >= target) {
            break;
        }
        high *= 2.0;
        if high > 100.0 {
            return Err("Fit needs more than 100 A RMS of equivalent return current".into());
        }
    }
    let p_high = evaluate(high)?;
    if (decreasing && p_high > target) || (!decreasing && p_high < target) {
        return Err("Measured P cannot be bracketed by the equivalent return model".into());
    }
    for _ in 0..16 {
        let mid = 0.5 * (low + high);
        let p = evaluate(mid)?;
        if (p - target).abs() < 0.05 {
            low = mid;
            high = mid;
            break;
        }
        if (decreasing && p > target) || (!decreasing && p < target) {
            low = mid;
        } else {
            high = mid;
        }
    }
    let current = 0.5 * (low + high);
    let driver = AcReturn {
        current_rms_a: current,
        phase_deg: phase,
    };
    let report = FittedReturn {
        baseline_active_power_w: baseline_power,
        target_active_power_w: target,
        baseline_conductance_s: if observed.location == Location::Port {
            baseline.input_admittance_real_s
        } else { baseline.source_power_w/circuit.voltage_rms.powi(2) },
        fitted_current_rms_a: current, fitted_phase_deg: phase,
        baseline_metrics: baseline,
        interpretation: "Equivalent AC-side powered return fitted to ONE reported active-power point; not a measured switching waveform or proof of the physical mechanism. Unmeasured Q and PF are not fitted.".into(),
    };
    Ok((driver, report))
}

pub fn run(config: Config, progress: impl Fn(usize, usize) + Sync + Send) -> Result<Sweep, String> {
    config.validate()?;
    let mut effective_circuit = config.circuit.clone();
    let fitted_return = if config.fit_to_observed_power {
        let (driver, report) = fit_return(&config)?;
        effective_circuit.ac_return = Some(driver);
        Some(report)
    } else {
        None
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.workers())
        .build()
        .map_err(|e| e.to_string())?;
    let total = config.points;
    let completed = std::sync::atomic::AtomicUsize::new(0);
    let results = pool.install(|| {
        (0..total)
            .into_par_iter()
            .map(|index| {
                let t = index as f64 / (total - 1) as f64;
                let cap = config.capacitance_min_uf
                    * (config.capacitance_max_uf / config.capacitance_min_uf).powf(t);
                let result = solve_checked(&effective_circuit, cap, config.reference_ohms)
                    .map_err(|e| format!("C1=C2={cap:.6} µF: {e}"));
                progress(
                    completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
                    total,
                );
                result
            })
            .collect::<Vec<_>>()
    });
    let recommended_index = results
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            row.as_ref()
                .ok()
                .filter(|r| r.metrics.quality_pass)
                .and_then(|r| r.metrics.reflection_magnitude.map(|g| (i, g)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    let observation_comparison = config.observed_ac.as_ref().and_then(|o| {
        results
            .iter()
            .filter_map(|row| row.as_ref().ok())
            .min_by(|a, b| {
                (a.metrics.capacitance_each_uf / o.capacitance_each_uf)
                    .ln()
                    .abs()
                    .total_cmp(
                        &(b.metrics.capacitance_each_uf / o.capacitance_each_uf)
                            .ln()
                            .abs(),
                    )
            })
            .map(|result| o.compare(&config.circuit, &result.metrics))
    });
    let workers_used = config.workers();
    let recorded_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    Ok(Sweep {
        config,
        effective_circuit,
        results,
        recommended_index,
        observation_comparison,
        fitted_return,
        workers_used,
        engine: format!(
            "diode-plug-lab Rust {} / implicit midpoint / periodic shooting",
            env!("CARGO_PKG_VERSION")
        ),
        recorded_unix_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::LoadModel;
    #[test]
    fn workers_do_not_exceed_available_sweep_points() {
        let mut config = crate::presets::all()[3].config.clone();
        config.points = 2;
        config.workers = 8;
        assert_eq!(config.workers(), 2);
        let result = run(config, |_, _| {}).unwrap();
        assert_eq!(result.workers_used, 2);
        config = result.config;
        config.workers = 1;
        assert_eq!(config.workers(), 1);
        config.workers = 0;
        assert!(config.workers() <= 2);
    }
    #[test]
    fn parallel_sweep_preserves_order_and_recommendation() {
        let config = Config {
            circuit: Circuit {
                voltage_rms: 24.0,
                frequency: 50.0,
                resistance: 100.0,
                inductance: 0.0,
                load_capacitance: 0.0,
                source_resistance: 2.0,
                diode_drop: 0.7,
                diode_resistance: 1.0,
                load_model: LoadModel::Linear,
                ac_return: None,
                ..Default::default()
            },
            preset_name: None,
            capacitance_min_uf: 1.0,
            capacitance_max_uf: 100.0,
            points: 4,
            reference_ohms: Some(50.0),
            observed_ac: None,
            fit_to_observed_power: false,
            operator: "tester".into(),
            run_id: "sample".into(),
            workers: 2,
        };
        let result = run(config, |_, _| {}).unwrap();
        assert_eq!(result.workers_used, 2);
        assert_eq!(result.results.len(), 4);
        assert!(result.results.iter().all(Result::is_ok));
        let caps: Vec<_> = result
            .results
            .iter()
            .map(|r| r.as_ref().unwrap().metrics.capacitance_each_uf)
            .collect();
        assert!(caps.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(result.recommended_index.is_some());
    }
    #[test]
    fn reported_grid_power_fit_retains_positive_passive_baseline() {
        let config = crate::presets::all()
            .into_iter()
            .find(|p| p.config.fit_to_observed_power)
            .unwrap()
            .config;
        let result = run(config, |_, _| {}).unwrap();
        result.config.validate().unwrap();
        let fit = result.fitted_return.as_ref().unwrap();
        let observed = result.config.observed_ac.as_ref().unwrap();
        let comparison = result.observation_comparison.as_ref().unwrap();
        assert!(fit.baseline_active_power_w > 0.0);
        assert!(fit.baseline_conductance_s > 0.0);
        assert!(result.config.circuit.ac_return.is_none());
        assert!(result.effective_circuit.ac_return.is_some());
        assert!(fit.fitted_current_rms_a > 0.0);
        assert!((comparison.simulated_active_power_w - observed.active_power_w).abs() < 0.1);
        assert!(comparison.simulated_conductance_s < 0.0);
        assert_eq!(comparison.observed_quadrant, None);
        assert!(result.results.iter().all(Result::is_ok));
    }

    #[test]
    fn hundred_volt_coil_sweep_records_refined_differences_without_hiding_checks() {
        let mut config = crate::presets::all()[9].config.clone();
        config.circuit.voltage_rms = 100.0;
        config.capacitance_min_uf = 0.001;
        config.capacitance_max_uf = 1.0;
        config.points = 16;
        config.reference_ohms = Some(50.0);
        let sweep = run(config, |_, _| {}).unwrap();
        assert_eq!(sweep.results.len(), 16);
        for point in sweep.results.iter().map(|r| r.as_ref().unwrap()) {
            let check = point.metrics.resolution_check.as_ref().unwrap();
            assert_eq!(check.refined_steps, 2 * check.base_steps);
            assert_eq!(check.differences_percent.len(), 6);
            assert_eq!(
                point.metrics.quality_pass,
                check.accepted_steps == Some(point.metrics.samples_per_period)
            );
        }
        let selected = &sweep.results[15].as_ref().unwrap().metrics;
        assert!((selected.source_power_w - 4643.56).abs() < 1.0);
        assert!(
            selected
                .resolution_check
                .as_ref()
                .unwrap()
                .max_difference_percent
                .unwrap()
                < 0.01
        );
        let new_check = &sweep.results[7].as_ref().unwrap().metrics;
        assert!(new_check.energy_balance_percent < 1.0);
        assert!(new_check.quality_pass);
        assert_eq!(new_check.samples_per_period, 1024);
        assert!(
            new_check
                .resolution_check
                .as_ref()
                .unwrap()
                .issues
                .iter()
                .any(|issue| issue.contains("port power changes"))
        );
        assert!(
            new_check
                .resolution_check
                .as_ref()
                .unwrap()
                .follow_up
                .as_ref()
                .unwrap()
                .issues
                .is_empty()
        );
        assert!(
            sweep
                .results
                .iter()
                .all(|r| r.as_ref().unwrap().metrics.quality_pass),
            "All points must pass both checks after adaptive refinement"
        );
        assert!(
            sweep.results[1]
                .as_ref()
                .unwrap()
                .metrics
                .samples_per_period
                > 2048
        );
    }

    #[test]
    fn adaptive_refinement_fails_closed_on_unstable_or_failed_grids() {
        let base = simulate(&Circuit::default(), 1.0, None).unwrap();
        let failed = stabilize_point(base.clone(), |_| Err("deliberately failed".into()));
        assert!(!failed.metrics.quality_pass);
        assert!(
            failed
                .metrics
                .resolution_check
                .as_ref()
                .unwrap()
                .trend
                .as_ref()
                .unwrap()
                .contains("failed")
        );
        let initial_steps = base.metrics.samples_per_period;
        let mut grids = Vec::new();
        let unstable = stabilize_point(base.clone(), |steps| {
            grids.push(steps);
            let mut point = base.clone();
            point.metrics.samples_per_period = steps;
            point.metrics.source_power_w += 10.0 * steps as f64 / initial_steps as f64;
            Ok(point)
        });
        assert!(!unstable.metrics.quality_pass);
        assert_eq!(unstable.metrics.samples_per_period, initial_steps);
        assert!(grids.len() > 2 && grids.last().copied().unwrap() <= 80_000);
        assert!(
            unstable
                .metrics
                .resolution_check
                .as_ref()
                .unwrap()
                .trend
                .as_ref()
                .unwrap()
                .contains("80,000")
        );
    }
}
