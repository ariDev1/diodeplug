use crate::solver::{Circuit, Metrics};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    #[default]
    Source,
    Port,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservedAc {
    pub location: Location,
    pub voltage_rms_v: f64,
    pub capacitance_each_uf: f64,
    pub active_power_w: f64,
    #[serde(default)]
    pub reactive_power_var: Option<f64>,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Comparison {
    pub observed_conductance_s: f64,
    pub observed_parallel_resistance_ohm: Option<f64>,
    pub observed_susceptance_s: Option<f64>,
    pub observed_signed_power_factor: Option<f64>,
    pub observed_quadrant: Option<u8>,
    pub simulation_capacitance_each_uf: f64,
    pub simulated_active_power_w: f64,
    pub simulated_conductance_s: f64,
    pub active_power_gap_w: f64,
    pub in_phase_current_gap_a: f64,
}

impl ObservedAc {
    pub fn validate(&self) -> Result<(), String> {
        if !self.voltage_rms_v.is_finite()
            || self.voltage_rms_v <= 0.0
            || !self.capacitance_each_uf.is_finite()
            || self.capacitance_each_uf <= 0.0
            || !self.active_power_w.is_finite()
            || self.reactive_power_var.is_some_and(|q| !q.is_finite())
        {
            return Err("Observation needs finite P, positive measured voltage and capacitor values, and finite Q when provided".into());
        }
        Ok(())
    }

    pub fn compare(&self, circuit: &Circuit, model: &Metrics) -> Comparison {
        let g = self.active_power_w / self.voltage_rms_v.powi(2);
        let (simulated_power, simulated_g) = match self.location {
            Location::Source => (
                model.source_power_w,
                model.source_power_w / circuit.voltage_rms.powi(2),
            ),
            Location::Port => (
                model.fundamental_active_power_w,
                model.input_admittance_real_s,
            ),
        };
        let gap = self.active_power_w - simulated_power;
        let q = self.reactive_power_var;
        Comparison {
            observed_conductance_s: g,
            observed_parallel_resistance_ohm: (g.abs() > 1e-15).then_some(1.0 / g),
            observed_susceptance_s: q.map(|q| -q / self.voltage_rms_v.powi(2)),
            observed_signed_power_factor: q
                .map(|q| self.active_power_w / self.active_power_w.hypot(q))
                .filter(|pf| pf.is_finite()),
            observed_quadrant: q.and_then(|q| match (self.active_power_w > 0.0, q > 0.0) {
                (true, true) => Some(1),
                (false, true) if self.active_power_w < 0.0 => Some(2),
                (false, false) if self.active_power_w < 0.0 && q < 0.0 => Some(3),
                (true, false) if q < 0.0 => Some(4),
                _ => None,
            }),
            simulation_capacitance_each_uf: model.capacitance_each_uf,
            simulated_active_power_w: simulated_power,
            simulated_conductance_s: simulated_g,
            active_power_gap_w: gap,
            in_phase_current_gap_a: gap / self.voltage_rms_v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{LoadModel, simulate};
    #[test]
    fn negative_active_power_does_not_imply_reactive_quadrant() {
        let observation = ObservedAc {
            location: Location::Source,
            voltage_rms_v: 240.0,
            capacitance_each_uf: 10.0,
            active_power_w: -29.0,
            reactive_power_var: None,
            note: "lab report".into(),
        };
        observation.validate().unwrap();
        assert!(
            (observation.active_power_w / observation.voltage_rms_v.powi(2) + 29.0 / 57600.0).abs()
                < 1e-15
        );
        let circuit = Circuit {
            voltage_rms: 240.0,
            frequency: 50.0,
            resistance: 2.0,
            inductance: 0.0,
            load_capacitance: 0.0,
            source_resistance: 0.1,
            diode_drop: 0.7,
            diode_resistance: 1.0,
            load_model: LoadModel::Linear,
            ac_return: None,
            ..Default::default()
        };
        let model = simulate(&circuit, 10.0, None).unwrap();
        assert!(
            model.metrics.energy_balance_percent < 1.0,
            "{:?}",
            model.metrics
        );
        let comparison = observation.compare(&circuit, &model.metrics);
        assert!(comparison.simulated_active_power_w > 0.0);
        assert!(comparison.active_power_gap_w < -29.0);
        assert!((comparison.observed_conductance_s + 29.0 / 57600.0).abs() < 1e-15);
        assert!(comparison.observed_parallel_resistance_ohm.unwrap() < 0.0);
        assert_eq!(comparison.observed_quadrant, None);
        assert_eq!(comparison.observed_signed_power_factor, None);
        let mut reported = observation.clone();
        reported.reactive_power_var = Some(-40.0);
        assert!(reported.validate().is_ok());
        let quadrant = reported.compare(&circuit, &model.metrics);
        assert_eq!(quadrant.observed_quadrant, Some(3));
        assert!(quadrant.observed_signed_power_factor.unwrap() < 0.0);
        assert!(quadrant.observed_susceptance_s.unwrap() > 0.0);
        assert!(
            ObservedAc {
                voltage_rms_v: 0.0,
                ..observation
            }
            .validate()
            .is_err()
        );
    }
}
