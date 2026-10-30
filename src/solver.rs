use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::f64::consts::{PI, SQRT_2};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadModel {
    #[default]
    Linear,
    /// Ideal blocking junction, knee + series resistance. Knee is the extrapolated zero-current voltage.
    Led { knee_voltage: f64 },
    /// Fixed open-circuit emf, internal resistance, ideal reverse-current blocking.
    Battery { open_circuit_voltage: f64 },
    /// Externally powered fixed EMF + R branch. Negative current supplies the P–N bus.
    BidirectionalBus { emf_voltage: f64 },
}

impl LoadModel {
    pub(crate) fn threshold(&self) -> f64 {
        match self {
            Self::Linear => 0.0,
            Self::Led { knee_voltage } => *knee_voltage,
            Self::Battery {
                open_circuit_voltage,
            } => *open_circuit_voltage,
            Self::BidirectionalBus { emf_voltage } => *emf_voltage,
        }
    }
}

/// External, powered, synchronous AC-port current injection representing a switched return path.
/// This is an explicit port-equivalent extension, not a passive diode-plug effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcReturn {
    pub current_rms_a: f64,
    pub phase_deg: f64,
}

/// Periodic load-branch gate (pulse-train load, phase-locked to t = 0 of the source).
/// While open, the load resistor/EMF and the parallel load capacitor are disconnected.
/// Only permitted for load branches with L = 0 H (an opened inductor has no defined state).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PulseGate {
    pub repetition_hz: f64,
    pub duty: f64,
}

impl PulseGate {
    fn level(&self, t: f64) -> f64 {
        let period = 1.0 / self.repetition_hz;
        if t.rem_euclid(period) < self.duty * period {
            1.0
        } else {
            0.0
        }
    }
}

impl Default for Circuit {
    fn default() -> Self {
        Self {
            voltage_rms: 24.0,
            frequency: 50.0,
            resistance: 100.0,
            inductance: 0.0,
            load_capacitance: 0.0,
            source_resistance: 1.0,
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
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Circuit {
    pub voltage_rms: f64,
    pub frequency: f64,
    pub resistance: f64,
    pub inductance: f64,
    pub load_capacitance: f64,
    pub source_resistance: f64,
    pub diode_drop: f64,
    pub diode_resistance: f64,
    #[serde(default)]
    pub load_model: LoadModel,
    #[serde(default)]
    pub ac_return: Option<AcReturn>,
    /// Finite source-side series inductance Ls (requires diode capacitance > 0).
    #[serde(default)]
    pub source_inductance: f64,
    /// C2 = C1 * capacitor_ratio (1.0 = symmetric plug).
    #[serde(default = "one")]
    pub capacitor_ratio: f64,
    /// Series ESR per plug capacitor branch (loss-only accounting at the reported branch currents).
    #[serde(default)]
    pub capacitor_esr_ohm: f64,
    /// Parallel leakage resistance across each plug capacitor (None = none).
    #[serde(default)]
    pub capacitor_leakage_parallel_ohm: Option<f64>,
    /// Junction capacitance of each clamp diode; needed for a dynamic node A.
    #[serde(default)]
    pub diode_capacitance_f: f64,
    /// Reverse-recovery charge per diode turn-off event (declared dissipation at turn-off).
    #[serde(default)]
    pub diode_recovery_charge_nc: f64,
    #[serde(default)]
    pub pulse_gate: Option<PulseGate>,
}

fn one() -> f64 {
    1.0
}

impl Circuit {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, positive) in [
            ("voltage_rms", self.voltage_rms, true),
            ("frequency", self.frequency, true),
            ("resistance", self.resistance, true),
            ("inductance", self.inductance, false),
            ("load_capacitance", self.load_capacitance, false),
            ("source_resistance", self.source_resistance, false),
            ("diode_drop", self.diode_drop, false),
            ("diode_resistance", self.diode_resistance, true),
        ] {
            if !value.is_finite() || value < 0.0 || (positive && value == 0.0) {
                return Err(format!(
                    "{name} must be finite and {}",
                    if positive { "positive" } else { "nonnegative" }
                ));
            }
        }
        if let LoadModel::Led { knee_voltage } = &self.load_model
            && (!knee_voltage.is_finite() || *knee_voltage < 0.0)
        {
            return Err("LED knee voltage must be finite and nonnegative".into());
        }
        if let LoadModel::Battery {
            open_circuit_voltage,
        } = &self.load_model
            && (!open_circuit_voltage.is_finite() || *open_circuit_voltage < 0.0)
        {
            return Err("Battery open-circuit voltage must be finite and nonnegative".into());
        }
        if let LoadModel::BidirectionalBus { emf_voltage } = &self.load_model
            && (!emf_voltage.is_finite() || *emf_voltage < 0.0)
        {
            return Err("Powered-bus EMF must be finite and nonnegative".into());
        }
        if self.load_model != LoadModel::Linear && self.inductance != 0.0 {
            return Err("LED and battery/bus models require L=0 H".into());
        }
        if let Some(driver) = &self.ac_return
            && (!driver.current_rms_a.is_finite()
                || driver.current_rms_a < 0.0
                || !driver.phase_deg.is_finite())
        {
            return Err("AC return needs nonnegative finite RMS current and finite phase".into());
        }
        for (name, value, positive) in [
            ("source_inductance", self.source_inductance, false),
            ("capacitor_ratio", self.capacitor_ratio, true),
            ("capacitor_esr", self.capacitor_esr_ohm, false),
            ("diode_capacitance", self.diode_capacitance_f, false),
            (
                "diode_recovery_charge",
                self.diode_recovery_charge_nc,
                false,
            ),
        ] {
            if !value.is_finite() || value < 0.0 || (positive && value == 0.0) {
                return Err(format!(
                    "{name} must be finite and {}",
                    if positive { "positive" } else { "nonnegative" }
                ));
            }
        }
        if let Some(leak) = self.capacitor_leakage_parallel_ohm
            && (!leak.is_finite() || leak <= 0.0)
        {
            return Err("Capacitor leakage resistance must be finite and positive".into());
        }
        if self.source_inductance > 0.0 && self.diode_capacitance_f == 0.0 {
            return Err(
                "Source inductance Ls requires a diode junction capacitance > 0: with an ideal \
                 series L the clamp-node potential (node A) has no defined shunt path while the \
                 diodes are off."
                    .into(),
            );
        }
        if let Some(gate) = &self.pulse_gate {
            if !gate.repetition_hz.is_finite() || gate.repetition_hz <= 0.0 {
                return Err("Pulse gate repetition must be finite and positive".into());
            }
            if !gate.duty.is_finite() || !(0.0..=1.0).contains(&gate.duty) {
                return Err("Pulse gate duty must be within (0, 1]".into());
            }
            if self.inductance != 0.0 {
                return Err("Pulse gating requires load L = 0 H".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Trace {
    pub time_s: f64,
    pub source_v: f64,
    pub port_v: f64,
    pub node_p_v: f64,
    pub node_n_v: f64,
    pub load_v: f64,
    pub load_a: f64,
    pub input_a: f64,
    pub plug_input_a: f64,
    pub return_injection_a: f64,
    pub return_path_power_w: f64,
    pub diode_1_a: f64,
    pub diode_2_a: f64,
    pub capacitor_1_a: f64,
    pub capacitor_2_a: f64,
    pub load_capacitor_a: f64,
    pub input_power_w: f64,
    pub port_power_w: f64,
    pub load_power_w: f64,
    pub capacitor_1_energy_j: f64,
    pub capacitor_2_energy_j: f64,
    pub load_capacitor_energy_j: f64,
    pub load_inductor_energy_j: f64,
    pub source_coil_a: f64,
    pub source_coil_energy_j: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Metrics {
    pub capacitance_each_uf: f64,
    pub samples_per_period: usize,
    pub convergence_method: String,
    pub cycles_integrated: usize,
    pub load_voltage_rms_v: f64,
    pub load_voltage_mean_v: f64,
    pub load_current_rms_a: f64,
    pub load_power_w: f64,
    pub load_resistor_power_w: f64,
    pub load_threshold_power_w: f64,
    pub input_current_rms_a: f64,
    pub source_power_w: f64,
    pub port_power_w: f64,
    pub source_negative_power_fraction: f64,
    pub port_negative_power_fraction: f64,
    pub source_returned_energy_per_cycle_j: f64,
    pub port_returned_energy_per_cycle_j: f64,
    pub external_bus_supplied_power_w: f64,
    pub return_path_supplied_power_w: f64,
    pub return_path_absorbed_power_w: f64,
    pub losses_w: f64,
    pub diode_loss_w: f64,
    pub source_resistor_loss_w: f64,
    pub input_peak_a: f64,
    pub diode_1_peak_a: f64,
    pub diode_2_peak_a: f64,
    pub diode_1_conduction_fraction: f64,
    pub diode_2_conduction_fraction: f64,
    pub capacitor_energy_peak_j: f64,
    pub capacitor_energy_swing_j: f64,
    pub input_energy_per_cycle_j: f64,
    pub load_energy_per_cycle_j: f64,
    pub fundamental_active_power_w: f64,
    pub fundamental_reactive_power_var: f64,
    pub input_current_thd: f64,
    pub energy_balance_error_w: f64,
    pub energy_balance_percent: f64,
    pub input_admittance_real_s: f64,
    pub input_admittance_imag_s: f64,
    pub plug_only_admittance_real_s: f64,
    pub plug_only_admittance_imag_s: f64,
    pub equivalent_parallel_resistance_ohm: Option<f64>,
    pub input_impedance_real_ohm: f64,
    pub input_impedance_imag_ohm: f64,
    pub input_voltage_fundamental_rms_v: f64,
    pub input_current_fundamental_rms_a: f64,
    pub reflection_magnitude: Option<f64>,
    pub quality_pass: bool,
    /// Empty when independent waveform/metric consistency checks pass.
    pub calculation_checks: Vec<String>,
    /// Present for sweep points independently rerun at twice the time resolution.
    pub resolution_check: Option<crate::validation::ResolutionCheck>,
    pub capacitance_2_uf: f64,
    pub capacitor_esr_loss_w: f64,
    pub capacitor_leakage_loss_w: f64,
    pub reverse_recovery_loss_w: f64,
    pub source_coil_current_rms_a: f64,
    pub source_coil_energy_swing_j: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResultPoint {
    pub metrics: Metrics,
    pub waveform: Vec<Trace>,
}

type State = [f64; 6];

/// Numerator index layout inside `State` for each solver mode.
#[derive(Clone, Copy)]
struct Layout {
    il: Option<usize>,   // load-branch inductor current (only when L > 0)
    coil: Option<usize>, // node-A series coil: source inductor current or tank inductor current
    va: Option<usize>,   // dynamic node A potential (only outside the stiff source mode)
}

#[derive(Clone, Copy, PartialEq)]
enum Drive {
    /// Original validated model: ideal voltage at node A set by the source algebra (2–3 states).
    Stiff,
    /// Dynamic node A driven by an ideal source behind Rs and, when with_l, in series Ls.
    Source { with_l: bool },
    /// Free-ringing parallel tank (L, C, loss G) connected to node A through the plug load.
    Tank { l: f64, c: f64, g: f64 },
}

fn layout_for(mode: Drive, circuit: &Circuit) -> (Layout, usize) {
    let has_il = circuit.inductance > 0.0;
    match mode {
        Drive::Stiff => (
            Layout {
                il: has_il.then_some(2),
                coil: None,
                va: None,
            },
            if has_il { 3 } else { 2 },
        ),
        Drive::Source { with_l } => {
            let mut next = 2;
            let il = has_il.then(|| {
                let i = next;
                next += 1;
                i
            });
            let coil = with_l.then(|| {
                let i = next;
                next += 1;
                i
            });
            let va = next;
            (
                Layout {
                    il,
                    coil,
                    va: Some(va),
                },
                next + 1,
            )
        }
        Drive::Tank { .. } => {
            let mut next = 2;
            let il = has_il.then(|| {
                let i = next;
                next += 1;
                i
            });
            let va = next;
            next += 1;
            let coil = next;
            (
                Layout {
                    il,
                    coil: Some(coil),
                    va: Some(va),
                },
                next + 1,
            )
        }
    }
}

fn solve3(m: &[[f64; 3]; 3], rhs: [f64; 3]) -> Option<[f64; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-300 || !det.is_finite() {
        return None;
    }
    let inv = 1.0 / det;
    let col = |k: usize| {
        let mut a = *m;
        for row in 0..3 {
            a[row][k] = rhs[row];
        }
        let d = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        d * inv
    };
    Some([col(0), col(1), col(2)])
}

#[derive(Clone, Copy)]
struct Instant {
    vs: f64,
    va: f64,
    il: f64,
    input: f64,
    plug_input: f64,
    return_injection: f64,
    d1: f64,
    d2: f64,
    dx: f64,
    dy: f64,
    coil_a: f64,
}

struct Context<'a> {
    circuit: &'a Circuit,
    plug1: f64,
    plug2: f64,
    n: usize,
    layout: Layout,
    mode: Drive,
    period: f64,
    steps: usize,
    phase_offset: f64,
}

impl Context<'_> {
    /// Algebraic KCL at stiff node A: the source sets the potential, the active-set of the
    /// diodes is chosen so that the resulting branch currents are mutually consistent.
    /// (Equivalent to `operating()` for leakage-free circuits; leakage currents only shift `b`.)
    fn operating(&self, vs: f64, x: f64, y: f64, _gate: f64) -> (f64, f64, f64, f64) {
        let c = self.circuit;
        let rd = c.diode_resistance;
        let vf = c.diode_drop;
        let leak = c.capacitor_leakage_parallel_ohm.map_or(0.0, |r| 1.0 / r) * (x + y);
        let alpha = 1.0 / rd;
        let rs = c.source_resistance;
        // Plug current as seen at node A, per active-set: i_plug(va) = a*va + b.
        let combos = [
            (false, false, 0.0, -leak),
            (true, false, alpha, (vf - x) / rd - leak),
            (false, true, alpha, -(y + vf) / rd - leak),
            (true, true, 2.0 * alpha, (vf - x - y - vf) / rd - leak),
        ];
        for (one, two, a, b) in combos {
            let va = if a > 0.0 {
                (vs / rs - b) / (a + 1.0 / rs)
            } else {
                // both off: (vs - va)/rs + leak = 0  ->  va = vs + rs*leak
                vs + rs * leak
            };
            let u1 = x - va - vf;
            let u2 = va - y - vf;
            if ((u1 > 0.0) != one && u1.abs() > 1e-9) || ((u2 > 0.0) != two && u2.abs() > 1e-9) {
                continue;
            }
            let d1 = ((x - va - vf) / rd).max(0.0);
            let d2 = ((va - y - vf) / rd).max(0.0);
            return (va, d1, d2, d2 - d1);
        }
        // The source/diode algebraic relation is monotone; this should be unreachable.
        unreachable!("diode active-set inconsistency")
    }

    fn evaluate(&self, t: f64, s: State) -> Result<(State, Instant), String> {
        match self.mode {
            Drive::Stiff => Ok(self.evaluate_stiff(t, s)),
            _ => self.evaluate_dyn(t, s),
        }
    }

    /// Common load-branch quantities shared by both evaluators.
    /// Returns (il, will the load L state be used, diodes (d1, d2) when dynamic).
    fn load_current(&self, vl: f64, il_state: Option<f64>, gate: f64) -> f64 {
        let c = self.circuit;
        match il_state {
            Some(state) => state,
            None => {
                gate * match c.load_model {
                    LoadModel::Linear => vl / c.resistance,
                    LoadModel::BidirectionalBus { emf_voltage } => {
                        (vl - emf_voltage) / c.resistance
                    }
                    _ => ((vl - c.load_model.threshold()) / c.resistance).max(0.0),
                }
            }
        }
    }

    fn evaluate_stiff(&self, t: f64, s: State) -> (State, Instant) {
        let c = self.circuit;
        let phase = 2.0 * PI * c.frequency * t + self.phase_offset;
        let vs = SQRT_2 * c.voltage_rms * phase.sin();
        let return_injection = c.ac_return.as_ref().map_or(0.0, |driver| {
            SQRT_2 * driver.current_rms_a * (phase + driver.phase_deg.to_radians()).sin()
        });
        let gate = c.pulse_gate.as_ref().map_or(1.0, |g| g.level(t));
        let x = s[0];
        let y = s[1];
        let il_state = self.layout.il.map(|i| s[i]);
        let il = self.load_current(y - x, il_state, gate);
        let (va, d1, d2, plug_input) =
            self.operating(vs + c.source_resistance * return_injection, x, y, gate);
        let input = plug_input - return_injection;
        // Gated 2x2: the gate modulates the series R/L branch only; the
        // load capacitor stays connected (its stored charge would otherwise
        // vanish at turn-off and break energy conservation).
        let leak = c.capacitor_leakage_parallel_ohm.map_or(0.0, |r| 1.0 / r);
        let gcl = c.load_capacitance;
        let det = self.plug1 * self.plug2 + gcl * (self.plug1 + self.plug2);
        let r1 = il - d1 - x * leak;
        let r2 = d2 - il - y * leak;
        let dx = ((self.plug2 + gcl) * r1 + gcl * r2) / det;
        let dy = (gcl * r1 + (self.plug1 + gcl) * r2) / det;
        let di = if let Some(i) = self.layout.il {
            ((y - x) - c.resistance * s[i]) / c.inductance
        } else {
            0.0
        };
        (
            [dx, dy, di, 0.0, 0.0, 0.0],
            Instant {
                vs,
                va,
                il,
                input,
                plug_input,
                return_injection,
                d1,
                d2,
                dx,
                dy,
                coil_a: 0.0,
            },
        )
    }

    /// Dynamic node A: state vector [x, y, (il), (coil), va]; diode junction
    /// capacitance closes the clamp-node KCL. Node system for unknowns (dx, dy, dva):
    ///   P: (C1+gCl+Cj)·dx − gCl·dy − Cj·dva = il − d1 − x/Rl
    ///   N: −gCl·dx + (C2+gCl+Cj)·dy − Cj·dva = d2 − il − y/Rl
    ///   A: C1·dx + C2·dy = coil_injection (source) | + Ct·dva − iT − va·Gloss (tank)
    fn evaluate_dyn(&self, t: f64, s: State) -> Result<(State, Instant), String> {
        let c = self.circuit;
        let gate = c.pulse_gate.as_ref().map_or(1.0, |g| g.level(t));
        let x = s[0];
        let y = s[1];
        let va = s[self.layout.va.unwrap()];
        let il = self.load_current(y - x, self.layout.il.map(|i| s[i]), gate);
        let d1 = ((x - va - c.diode_drop) / c.diode_resistance).max(0.0);
        let d2 = ((va - y - c.diode_drop) / c.diode_resistance).max(0.0);
        let leak = c.capacitor_leakage_parallel_ohm.map_or(0.0, |r| 1.0 / r);
        let gcl = c.load_capacitance;
        let cj = c.diode_capacitance_f;
        let phase = 2.0 * PI * c.frequency * t + self.phase_offset;
        let return_injection = c.ac_return.as_ref().map_or(0.0, |driver| {
            SQRT_2 * driver.current_rms_a * (phase + driver.phase_deg.to_radians()).sin()
        });
        let vs = SQRT_2 * c.voltage_rms * phase.sin();
        let mut m = [[0.0f64; 3]; 3];
        m[0] = [self.plug1 + gcl + cj, -gcl, -cj];
        m[1] = [-gcl, self.plug2 + gcl + cj, -cj];
        let rhs3 = match self.mode {
            Drive::Tank { c: tc, g: loss, .. } => {
                let it = s[self.layout.coil.unwrap()];
                m[2] = [self.plug1, self.plug2, tc];
                return_injection - it - va * loss
            }
            _ => {
                m[2] = [self.plug1, self.plug2, 0.0];
                let coil = self
                    .layout
                    .coil
                    .map_or((vs - va) / c.source_resistance, |i| s[i]);
                coil + return_injection
            }
        };
        let [dx, dy, dva] = solve3(&m, [il - d1 - x * leak, d2 - il - y * leak, rhs3])
            .ok_or("Dynamic node equations are singular; increase diode capacitance or load cap")?;
        let coil_a = self
            .layout
            .coil
            .map_or((vs - va) / c.source_resistance, |i| s[i]);
        let input = match self.mode {
            // Tank drive meters the plug draw minus the tank's supply to node A.
            Drive::Tank { g: loss, .. } => (d2 - d1) - s[self.layout.coil.unwrap()] - va * loss,
            _ => coil_a,
        };
        let mut d = [0.0; 6];
        d[0] = dx;
        d[1] = dy;
        if let Some(i) = self.layout.il {
            d[i] = ((y - x) - c.resistance * s[i]) / c.inductance;
        }
        if let Some(i) = self.layout.coil {
            d[i] = match self.mode {
                Drive::Source { with_l: true } => {
                    (vs - c.source_resistance * s[i] - va) / c.source_inductance
                }
                Drive::Tank { l, .. } => va / l,
                _ => 0.0,
            };
        }
        d[self.layout.va.unwrap()] = dva;
        Ok((
            d,
            Instant {
                vs,
                va,
                il,
                input,
                plug_input: d2 - d1,
                return_injection,
                d1,
                d2,
                dx,
                dy,
                coil_a,
            },
        ))
    }

    // Implicit midpoint: A-stable, second order, no explicit small-Rd CFL restriction.
    fn step(&self, t: f64, old: State, dt: f64) -> Result<State, String> {
        self.step_adaptive(t, old, dt, 0)
    }

    fn step_adaptive(&self, t: f64, old: State, dt: f64, depth: usize) -> Result<State, String> {
        match self.step_once(t, old, dt) {
            Ok(next) => Ok(next),
            Err(_) if depth < 10 => {
                let half = self.step_adaptive(t, old, dt * 0.5, depth + 1)?;
                self.step_adaptive(t + dt * 0.5, half, dt * 0.5, depth + 1)
            }
            Err(error) => Err(error),
        }
    }

    fn step_once(&self, t: f64, old: State, dt: f64) -> Result<State, String> {
        let (f0, _) = self.evaluate(t, old)?;
        let mut next = old;
        for i in 0..self.n {
            next[i] += dt * f0[i];
        }
        let midpoint = t + dt * 0.5;
        for _ in 0..12 {
            let mid = midpoint_state(old, next);
            let (f, _) = self.evaluate(midpoint, mid)?;
            let mut residual = [0.0; 6];
            for i in 0..self.n {
                residual[i] = next[i] - old[i] - dt * f[i];
            }
            if (0..self.n).all(|i| residual[i].abs() < 1e-10 * (1.0 + next[i].abs())) {
                return Ok(next);
            }
            let mut jacobian = [[0.0; 6]; 6];
            for j in 0..self.n {
                let h = 1e-5 * (1.0 + next[j].abs());
                let mut trial = mid;
                trial[j] += h * 0.5;
                let (fj, _) = self.evaluate(midpoint, trial)?;
                for i in 0..self.n {
                    jacobian[i][j] = (if i == j { 1.0 } else { 0.0 }) - dt * (fj[i] - f[i]) / h;
                }
            }
            let delta = linear_solve(jacobian, residual.map(|x| -x), self.n)
                .ok_or("Singular implicit step")?;
            for i in 0..self.n {
                next[i] += delta[i];
            }
            if !next.iter().all(|v| v.is_finite()) {
                return Err("Nonfinite solver state".into());
            }
        }
        Err("Implicit step did not converge; reduce timestep".into())
    }

    fn integrate(&self, initial: State, capture: bool) -> Result<(State, Vec<State>), String> {
        let dt = self.period / self.steps as f64;
        let mut state = initial;
        let mut states = if capture {
            Vec::with_capacity(self.steps + 1)
        } else {
            Vec::new()
        };
        if capture {
            states.push(state);
        }
        for k in 0..self.steps {
            let t = k as f64 * dt;
            // The gate switch makes the load branch discontinuous; refine
            // any step that straddles a gate transition so the fast
            // junction/drain-cap exchanges across the edge are resolved.
            let straddles_gate = self
                .circuit
                .pulse_gate
                .as_ref()
                .is_some_and(|g| g.level(t) != g.level(t + dt))
                && self.mode != Drive::Stiff;
            if straddles_gate {
                const SUB: usize = 8;
                let sub = dt / SUB as f64;
                for j in 0..SUB {
                    state = self.step(t + j as f64 * sub, state, sub)?;
                }
            } else {
                state = self.step(t, state, dt)?;
            }
            if capture {
                states.push(state);
            }
        }
        Ok((state, states))
    }
}

/// Continuous time-domain integration from an initial zero-energy state. Unlike
/// `simulate`, this never shoots or settles to a periodic orbit. The simulation
/// clock advances only by integrated steps, not by wall-clock time.
pub struct Transient {
    circuit: Circuit,
    cap_uf: f64,
    state: State,
    time_s: f64,
    source_phase_offset: f64,
    history: VecDeque<Trace>,
    integration_steps: u64,
}

/// Retain more than the widest scope capture so crossings remain eligible
/// across background integration chunks.
pub const LIVE_HISTORY_SAMPLES: usize = 16_384;
const LIVE_SCOPE_SAMPLES_PER_CYCLE: usize = 3072;

fn transient_context(circuit: &Circuit, cap_uf: f64, phase_offset: f64) -> Context<'_> {
    let mode = if circuit.source_inductance > 0.0 {
        Drive::Source { with_l: true }
    } else if circuit.diode_capacitance_f > 0.0 {
        Drive::Source { with_l: false }
    } else {
        Drive::Stiff
    };
    let (layout, n) = layout_for(mode, circuit);
    // Resolution follows the same pulse/dynamic heuristics as the periodic solver.
    let base =
        if circuit.frequency < 1000.0 && circuit.voltage_rms >= 100.0 && circuit.resistance <= 10.0
        {
            8192
        } else if circuit.frequency < 1000.0 {
            2048
        } else if circuit.frequency == 1000.0 {
            1024
        } else {
            512
        };
    let cap = cap_uf * 1e-6;
    let pulse = (1.0
        / circuit.frequency
        / (2.0
            * cap
            * (circuit.source_resistance + circuit.diode_resistance + circuit.capacitor_esr_ohm)))
        .ceil() as usize;
    let dynamic = if mode == Drive::Stiff {
        0
    } else if circuit.pulse_gate.is_some() {
        8192
    } else {
        4096
    };
    Context {
        circuit,
        plug1: cap,
        plug2: cap * circuit.capacitor_ratio,
        n,
        layout,
        mode,
        period: 1.0 / circuit.frequency,
        steps: base.max(pulse).max(dynamic).min(20_000),
        phase_offset,
    }
}

impl Transient {
    pub fn new(circuit: Circuit, cap_uf: f64) -> Result<Self, String> {
        circuit.validate()?;
        if !cap_uf.is_finite() || cap_uf <= 0.0 {
            return Err("Live C1 must be positive and finite".into());
        }
        let mut run = Self {
            circuit,
            cap_uf,
            state: [0.0; 6],
            time_s: 0.0,
            source_phase_offset: 0.0,
            history: VecDeque::with_capacity(LIVE_HISTORY_SAMPLES),
            integration_steps: 0,
        };
        run.advance(0)?;
        Ok(run)
    }

    pub fn time_s(&self) -> f64 {
        self.time_s
    }
    pub fn capacitance_uf(&self) -> f64 {
        self.cap_uf
    }
    pub fn circuit(&self) -> &Circuit {
        &self.circuit
    }
    pub fn waveform(&self) -> Vec<Trace> {
        self.history.iter().cloned().collect()
    }

    /// Change parameters at the current simulated instant, retaining capacitor
    /// voltages and inductor currents. Changing an ideal component can inject or
    /// remove energy; the operator's edit is an external intervention, not a
    /// charge-conserving physical switch model.
    pub fn reconfigure(&mut self, circuit: Circuit, cap_uf: f64) -> Result<(), String> {
        circuit.validate()?;
        if !cap_uf.is_finite() || cap_uf <= 0.0 {
            return Err("Live C1 must be positive and finite".into());
        }
        let old = transient_context(&self.circuit, self.cap_uf, self.source_phase_offset);
        let (_, instant) = old.evaluate(self.time_s, self.state)?;
        let next_phase = (self.source_phase_offset
            + 2.0 * PI * (self.circuit.frequency - circuit.frequency) * self.time_s)
            .rem_euclid(2.0 * PI);
        let next = transient_context(&circuit, cap_uf, next_phase);
        let mut state = [0.0; 6];
        state[0] = self.state[0];
        state[1] = self.state[1];
        if let Some(i) = next.layout.il {
            state[i] = old.layout.il.map_or(instant.il, |j| self.state[j]);
        }
        if let Some(i) = next.layout.coil {
            state[i] = old.layout.coil.map_or(instant.coil_a, |j| self.state[j]);
        }
        if let Some(i) = next.layout.va {
            state[i] = instant.va;
        }
        next.evaluate(self.time_s, state)?;
        self.circuit = circuit;
        self.cap_uf = cap_uf;
        self.source_phase_offset = next_phase;
        self.state = state;
        Ok(())
    }

    pub fn advance(&mut self, steps: usize) -> Result<(), String> {
        if steps > 512 {
            return Err("Live integration chunk exceeds 512 steps".into());
        }
        let circuit = self.circuit.clone();
        let ctx = transient_context(&circuit, self.cap_uf, self.source_phase_offset);
        // Keep roughly 3k recorded scope points per slowest configured cycle,
        // including a slower pulse gate. The solver still takes every step.
        let gate_ratio = circuit.pulse_gate.as_ref().map_or(1.0, |gate| {
            (circuit.frequency / gate.repetition_hz).max(1.0)
        });
        let scope_stride =
            ((ctx.steps as f64 * gate_ratio) / LIVE_SCOPE_SAMPLES_PER_CYCLE as f64).ceil() as u64;
        let scope_stride = scope_stride.max(1);
        for _ in 0..steps.max(1) {
            let t = self.time_s;
            let (_, z) = ctx.evaluate(t, self.state)?;
            let x = self.state[0];
            let y = self.state[1];
            let il = ctx.layout.il;
            let coil = ctx.layout.coil;
            let load_a = z.il + circuit.load_capacitance * (z.dy - z.dx);
            let trace = Trace {
                time_s: t,
                source_v: z.vs,
                port_v: z.va,
                node_p_v: z.va - x,
                node_n_v: z.va - y,
                load_v: y - x,
                load_a,
                input_a: z.input,
                plug_input_a: z.plug_input,
                return_injection_a: z.return_injection,
                return_path_power_w: -z.va * z.return_injection,
                diode_1_a: z.d1,
                diode_2_a: z.d2,
                capacitor_1_a: ctx.plug1 * z.dx,
                capacitor_2_a: ctx.plug2 * z.dy,
                load_capacitor_a: circuit.load_capacitance * (z.dy - z.dx),
                input_power_w: z.vs * z.input,
                port_power_w: z.va * z.input,
                load_power_w: (y - x) * load_a,
                capacitor_1_energy_j: 0.5 * ctx.plug1 * x * x,
                capacitor_2_energy_j: 0.5 * ctx.plug2 * y * y,
                load_capacitor_energy_j: 0.5 * circuit.load_capacitance * (y - x).powi(2),
                load_inductor_energy_j: il
                    .map_or(0.0, |i| 0.5 * circuit.inductance * self.state[i].powi(2)),
                source_coil_a: z.coil_a,
                source_coil_energy_j: coil.map_or(0.0, |i| {
                    0.5 * circuit.source_inductance * self.state[i].powi(2)
                }),
            };
            if !trace.source_v.is_finite()
                || !trace.load_v.is_finite()
                || !trace.input_a.is_finite()
            {
                return Err("Nonfinite live waveform".into());
            }
            if (steps == 0 || self.integration_steps.is_multiple_of(scope_stride))
                && self.history.back().is_none_or(|last| last.time_s != t)
            {
                if self.history.len() == LIVE_HISTORY_SAMPLES {
                    self.history.pop_front();
                }
                self.history.push_back(trace);
            }
            if steps == 0 {
                break;
            }
            let dt = ctx.period / ctx.steps as f64;
            let straddles = circuit
                .pulse_gate
                .as_ref()
                .is_some_and(|g| g.level(t) != g.level(t + dt))
                && ctx.mode != Drive::Stiff;
            let next = if straddles {
                let mut state = self.state;
                for j in 0..8 {
                    state = ctx.step(t + j as f64 * dt / 8.0, state, dt / 8.0)?;
                }
                state
            } else {
                ctx.step(t, self.state, dt)?
            };
            self.state = next;
            self.time_s += dt;
            self.integration_steps += 1;
        }
        Ok(())
    }
}

fn midpoint_state(a: State, b: State) -> State {
    let mut m = [0.0; 6];
    for (m_i, (a_i, b_i)) in m.iter_mut().zip(a.iter().zip(b.iter())) {
        *m_i = (a_i + b_i) * 0.5;
    }
    m
}

fn linear_solve(mut a: [[f64; 6]; 6], mut b: State, n: usize) -> Option<State> {
    for k in 0..n {
        let pivot = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[pivot][k].abs() < 1e-14 {
            return None;
        }
        a.swap(k, pivot);
        b.swap(k, pivot);
        let pivot_row = a[k];
        for i in k + 1..n {
            let factor = a[i][k] / pivot_row[k];
            for (entry, top) in a[i][k..n].iter_mut().zip(pivot_row[k..n].iter()) {
                *entry -= factor * top;
            }
            b[i] -= factor * b[k];
        }
    }
    let mut x = [0.0; 6];
    for i in (0..n).rev() {
        x[i] = (b[i] - (i + 1..n).map(|j| a[i][j] * x[j]).sum::<f64>()) / a[i][i];
    }
    Some(x)
}

fn mismatch(ctx: &Context<'_>, state: State, scale: State) -> Result<State, String> {
    let (end, _) = ctx.integrate(state, false)?;
    Ok(std::array::from_fn(|i| (end[i] - state[i]) / scale[i]))
}

fn norm(v: State, n: usize) -> f64 {
    (0..n).map(|i| v[i].abs()).fold(0.0, f64::max)
}

fn shooting(ctx: &Context<'_>, start: State, scale: State) -> Result<Option<State>, String> {
    let mut guess = start;
    for _ in 0..28 {
        let residual = mismatch(ctx, guess, scale)?;
        let error = norm(residual, ctx.n);
        if error < 2e-7 {
            return Ok(Some(guess));
        }
        let mut jacobian = [[0.0; 6]; 6];
        for j in 0..ctx.n {
            // A finite perturbation larger than the integrator's switching noise.
            let h = 0.005 * scale[j].max(guess[j].abs());
            let mut trial = guess;
            trial[j] += h;
            let other = mismatch(ctx, trial, scale)?;
            for i in 0..ctx.n {
                jacobian[i][j] = (other[i] - residual[i]) / (h / scale[j]);
            }
        }
        let Some(delta) = linear_solve(jacobian, residual.map(|v| -v), ctx.n) else {
            return Ok(None);
        };
        let mut improved = false;
        for power in 0..9 {
            let alpha = 2.0_f64.powi(-power);
            let mut trial = guess;
            for i in 0..ctx.n {
                trial[i] += alpha * delta[i] * scale[i];
            }
            if !trial.iter().all(|v| v.is_finite()) {
                continue;
            }
            if norm(mismatch(ctx, trial, scale)?, ctx.n) < error {
                guess = trial;
                improved = true;
                break;
            }
        }
        if !improved {
            return Ok(None);
        }
    }
    Ok(None)
}

pub fn simulate(
    circuit: &Circuit,
    capacitance_uf: f64,
    reference: Option<f64>,
) -> Result<ResultPoint, String> {
    simulate_with_steps(circuit, capacitance_uf, reference, None)
}

pub(crate) fn simulate_refined(
    circuit: &Circuit,
    capacitance_uf: f64,
    reference: Option<f64>,
    steps: usize,
) -> Result<ResultPoint, String> {
    if !(2..=80_000).contains(&steps) {
        return Err("Refined sampling must be between 2 and 80,000 steps per period".into());
    }
    simulate_with_steps(circuit, capacitance_uf, reference, Some(steps))
}

fn simulate_with_steps(
    circuit: &Circuit,
    capacitance_uf: f64,
    reference: Option<f64>,
    steps_override: Option<usize>,
) -> Result<ResultPoint, String> {
    circuit.validate()?;
    if !capacitance_uf.is_finite() || capacitance_uf <= 0.0 {
        return Err("C1=C2 must be positive".into());
    }
    if reference.is_some_and(|z| !z.is_finite() || z <= 0.0) {
        return Err("Z0 must be positive".into());
    }
    let cap = capacitance_uf * 1e-6;
    let c2 = cap * circuit.capacitor_ratio;
    // Narrow diode pulses need enough points for RMS and power quadrature,
    // even when an implicit integrator is stable at a larger step.
    let base_steps =
        if circuit.frequency < 1000.0 && circuit.voltage_rms >= 100.0 && circuit.resistance <= 10.0
        {
            8192
        } else if circuit.frequency < 1000.0 {
            2048
        } else if circuit.frequency == 1000.0 {
            1024
        } else {
            512
        };
    let pulse_steps = (1.0
        / circuit.frequency
        / (2.0
            * cap
            * (circuit.source_resistance + circuit.diode_resistance + circuit.capacitor_esr_ohm)))
        .ceil() as usize;
    let mode = match (
        circuit.source_inductance > 0.0,
        circuit.diode_capacitance_f > 0.0,
    ) {
        (true, _) => Drive::Source { with_l: true },
        (false, true) => Drive::Source { with_l: false },
        (false, false) => Drive::Stiff,
    };
    let (layout, n) = layout_for(mode, circuit);
    let dynamic_steps = if mode == Drive::Stiff {
        0
    } else if circuit.pulse_gate.is_some() {
        8192
    } else {
        4096
    };
    let ctx = Context {
        circuit,
        plug1: cap,
        plug2: c2,
        layout,
        mode,
        n,
        period: 1.0 / circuit.frequency,
        steps: steps_override
            .unwrap_or_else(|| base_steps.max(pulse_steps).max(dynamic_steps).min(20_000)),
        phase_offset: 0.0,
    };
    let (c1v, c2v, ilv, coil, vai) = (0, 1, layout.il, layout.coil, layout.va);
    let scale: [f64; 6] = {
        let mut s = [1.0; 6];
        s[c1v] = circuit.voltage_rms.max(1.0);
        s[c2v] = circuit.voltage_rms.max(1.0);
        if let Some(i) = ilv {
            s[i] = (circuit.voltage_rms / circuit.resistance).max(1e-3);
        }
        if let Some(i) = coil {
            s[i] = (circuit.voltage_rms / circuit.source_resistance.max(1.0)).max(1e-3);
        }
        if let Some(i) = vai {
            s[i] = circuit.voltage_rms.max(1.0);
        }
        s
    };
    let mut state = [0.0; 6];
    let mut method = "periodic shooting";
    let mut cycles = 0;
    let mut found = None;
    for cycle in 0..6000 {
        let (end, _) = ctx.integrate(state, false)?;
        cycles += 1;
        if cycle > 0
            && norm(
                std::array::from_fn(|i| (end[i] - state[i]) / scale[i]),
                ctx.n,
            ) < 2e-7
        {
            found = Some(state);
            method = "natural settling";
            break;
        }
        state = end;
        if [2, 20, 100, 500].contains(&cycle)
            && let Some(periodic) = shooting(&ctx, state, scale)?
        {
            found = Some(periodic);
            break;
        }
    }
    let start = found.ok_or(format!(
        "C1=C2={capacitance_uf:.6} µF did not reach periodic state after 6000 cycles"
    ))?;
    let (end, states) = ctx.integrate(start, true)?;
    let drift = norm(
        std::array::from_fn(|i| (end[i] - start[i]) / scale[i]),
        ctx.n,
    );
    if drift > 1e-5 {
        return Err(format!("Periodic state residual too large ({drift:.3e})"));
    }
    let dt = ctx.period / ctx.steps as f64;
    let mut waveform = Vec::with_capacity(ctx.steps + 1);
    let mut integral = [0.0_f64; 22];
    let mut vr = (0.0, 0.0);
    let mut ir = (0.0, 0.0);
    let mut plug_ir = (0.0, 0.0);
    let mut cap_min = f64::INFINITY;
    let mut cap_max: f64 = 0.0;
    let mut coil_energy_min = f64::INFINITY;
    let mut coil_energy_max: f64 = 0.0;
    let mut input_peak: f64 = 0.0;
    let mut d1_peak: f64 = 0.0;
    let mut d2_peak: f64 = 0.0;
    let mut d1_prev: f64 = 0.0;
    let mut d2_prev: f64 = 0.0;
    let mut recovery_pulse_j: f64 = 0.0;
    for (k, &s) in states.iter().enumerate() {
        let time = k as f64 * dt;
        let (_, z) = ctx.evaluate(time, s)?;
        let load_a = z.il + circuit.load_capacitance * (z.dy - z.dx);
        let c1 = cap;
        let e1 = 0.5 * c1 * s[c1v].powi(2);
        let e2 = 0.5 * c2 * s[c2v].powi(2);
        cap_min = cap_min.min(e1 + e2);
        cap_max = cap_max.max(e1 + e2);
        let source_v = z.vs;
        let va_now = z.va;
        let x_now = s[c1v];
        let y_now = s[c2v];
        let coil_energy = coil.map_or(0.0, |i| match mode {
            Drive::Source { with_l: true } => 0.5 * circuit.source_inductance * s[i] * s[i],
            Drive::Tank { l, .. } => 0.5 * l * s[i].powi(2),
            _ => 0.0,
        });
        coil_energy_min = coil_energy_min.min(coil_energy);
        coil_energy_max = coil_energy_max.max(coil_energy);
        input_peak = input_peak.max(z.input.abs());
        d1_peak = d1_peak.max(z.d1);
        d2_peak = d2_peak.max(z.d2);
        // Reverse-recovery charge is dissipated when a conducting diode snaps off.
        if d1_prev > 1e-12 && z.d1 <= 1e-12 {
            recovery_pulse_j += circuit.diode_recovery_charge_nc * 1e-9 * (x_now - va_now).max(0.0);
        }
        if d2_prev > 1e-12 && z.d2 <= 1e-12 {
            recovery_pulse_j += circuit.diode_recovery_charge_nc * 1e-9 * (va_now - y_now).max(0.0);
        }
        d1_prev = z.d1;
        d2_prev = z.d2;
        let tank_cap_energy = match mode {
            Drive::Tank { c: tc, .. } => 0.5 * tc * s[vai.unwrap()].powi(2),
            _ => 0.0,
        };
        let trace = Trace {
            time_s: time,
            source_v,
            port_v: va_now,
            node_p_v: va_now - x_now,
            node_n_v: va_now - y_now,
            load_v: y_now - x_now,
            load_a,
            input_a: z.input,
            plug_input_a: z.plug_input,
            return_injection_a: z.return_injection,
            return_path_power_w: -va_now * z.return_injection,
            diode_1_a: z.d1,
            diode_2_a: z.d2,
            capacitor_1_a: c1 * z.dx,
            capacitor_2_a: c2 * z.dy,
            load_capacitor_a: circuit.load_capacitance * (z.dy - z.dx),
            input_power_w: source_v * z.input,
            port_power_w: va_now * z.input,
            load_power_w: (y_now - x_now) * load_a,
            capacitor_1_energy_j: e1,
            capacitor_2_energy_j: e2,
            load_capacitor_energy_j: 0.5 * circuit.load_capacitance * (y_now - x_now).powi(2),
            load_inductor_energy_j: ilv.map_or(0.0, |i| 0.5 * circuit.inductance * s[i].powi(2)),
            source_coil_a: z.coil_a,
            source_coil_energy_j: coil_energy,
        };
        let w = if k == 0 || k == ctx.steps {
            0.5 / ctx.steps as f64
        } else {
            1.0 / ctx.steps as f64
        };
        let values = [
            (y_now - x_now) * (y_now - x_now),
            y_now - x_now,
            load_a * load_a,
            circuit.resistance * z.il * z.il,
            z.input * z.input,
            source_v * z.input,
            va_now * z.input,
            circuit.source_resistance * z.input * z.input
                + circuit.diode_drop * (z.d1 + z.d2)
                + circuit.diode_resistance * (z.d1 * z.d1 + z.d2 * z.d2),
            circuit.load_model.threshold() * z.il,
            circuit.diode_drop * (z.d1 + z.d2)
                + circuit.diode_resistance * (z.d1 * z.d1 + z.d2 * z.d2),
            circuit.source_resistance * z.input * z.input,
            if z.d1 > 1e-10 { 1.0 } else { 0.0 },
            if z.d2 > 1e-10 { 1.0 } else { 0.0 },
            if source_v * z.input < 0.0 { 1.0 } else { 0.0 },
            if va_now * z.input < 0.0 { 1.0 } else { 0.0 },
            (-(source_v * z.input)).max(0.0),
            (-(va_now * z.input)).max(0.0),
            -va_now * z.return_injection,
            // ESR dissipation on the plug capacitor branches.
            circuit.capacitor_esr_ohm * ((c1 * z.dx).powi(2) + (c2 * z.dy).powi(2)),
            // Leakage across each plug capacitor.
            (x_now * x_now + y_now * y_now)
                * circuit
                    .capacitor_leakage_parallel_ohm
                    .map_or(0.0, |r| 1.0 / r),
            // Source/tank coil copper current squared (for RMS).
            z.coil_a * z.coil_a,
            // Tank capacitor energy for swing accounting.
            tank_cap_energy,
        ];
        for (acc, val) in integral.iter_mut().zip(values) {
            *acc += w * val;
        }
        let phase = 2.0 * PI * time / ctx.period;
        vr.0 += w * 2.0 * va_now * phase.cos();
        vr.1 -= w * 2.0 * va_now * phase.sin();
        ir.0 += w * 2.0 * z.input * phase.cos();
        ir.1 -= w * 2.0 * z.input * phase.sin();
        plug_ir.0 += w * 2.0 * z.plug_input * phase.cos();
        plug_ir.1 -= w * 2.0 * z.plug_input * phase.sin();
        waveform.push(trace);
    }
    let vmag2 = vr.0 * vr.0 + vr.1 * vr.1;
    let imag2 = ir.0 * ir.0 + ir.1 * ir.1;
    if vmag2 < 1e-20 || imag2 < 1e-20 {
        return Err("Input fundamental too small to calculate impedance".into());
    }
    let yr = (ir.0 * vr.0 + ir.1 * vr.1) / vmag2;
    let yi = (ir.1 * vr.0 - ir.0 * vr.1) / vmag2;
    let plug_yr = (plug_ir.0 * vr.0 + plug_ir.1 * vr.1) / vmag2;
    let plug_yi = (plug_ir.1 * vr.0 - plug_ir.0 * vr.1) / vmag2;
    let zr = (vr.0 * ir.0 + vr.1 * ir.1) / imag2;
    let zi = (vr.1 * ir.0 - vr.0 * ir.1) / imag2;
    let energy = |s: State| {
        let va_e = vai.map_or(0.0, |i| s[i]);
        let il_e = ilv.map_or(0.0, |i| 0.5 * circuit.inductance * s[i] * s[i]);
        let coil_e = coil.map_or(0.0, |i| match mode {
            Drive::Source { with_l: true } => 0.5 * circuit.source_inductance * s[i] * s[i],
            Drive::Tank { l, .. } => 0.5 * l * s[i] * s[i],
            _ => 0.0,
        });
        let tank_cap_e = match mode {
            Drive::Tank { c: tc, .. } => 0.5 * tc * va_e * va_e,
            _ => 0.0,
        };
        let junction_e = if mode == Drive::Stiff {
            0.0
        } else {
            0.5 * circuit.diode_capacitance_f * ((s[c1v] - va_e).powi(2) + (s[c2v] - va_e).powi(2))
        };
        0.5 * cap * s[c1v] * s[c1v]
            + 0.5 * c2 * s[c2v] * s[c2v]
            + 0.5 * circuit.load_capacitance * (s[c2v] - s[c1v]).powi(2)
            + il_e
            + coil_e
            + tank_cap_e
            + junction_e
    };
    let load_power = integral[3] + integral[8];
    let esr_loss = integral[18];
    let leak_loss = integral[19];
    let recovery_loss = recovery_pulse_j / ctx.period;
    let balance = integral[5]
        - load_power
        - integral[7]
        - integral[17]
        - esr_loss
        - leak_loss
        - recovery_loss
        - (energy(end) - energy(start)) / ctx.period;
    let balance_percent = 100.0 * balance.abs() / integral[5].abs().max(load_power.abs()).max(1e-3);
    let reflection =
        reference.map(|z0| (((zr - z0).powi(2) + zi * zi) / ((zr + z0).powi(2) + zi * zi)).sqrt());
    let metrics = Metrics {
        capacitance_each_uf: capacitance_uf,
        samples_per_period: ctx.steps,
        convergence_method: method.into(),
        cycles_integrated: cycles,
        load_voltage_rms_v: integral[0].max(0.0).sqrt(),
        load_voltage_mean_v: integral[1],
        load_current_rms_a: integral[2].max(0.0).sqrt(),
        load_power_w: load_power,
        load_resistor_power_w: integral[3],
        load_threshold_power_w: integral[8],
        input_current_rms_a: integral[4].max(0.0).sqrt(),
        source_power_w: integral[5],
        port_power_w: integral[6],
        source_negative_power_fraction: integral[13],
        port_negative_power_fraction: integral[14],
        source_returned_energy_per_cycle_j: integral[15] * ctx.period,
        port_returned_energy_per_cycle_j: integral[16] * ctx.period,
        external_bus_supplied_power_w: (-integral[8]).max(0.0),
        return_path_supplied_power_w: (-integral[17]).max(0.0),
        return_path_absorbed_power_w: integral[17],
        losses_w: integral[7],
        diode_loss_w: integral[9],
        source_resistor_loss_w: integral[10],
        input_peak_a: input_peak,
        diode_1_peak_a: d1_peak,
        diode_2_peak_a: d2_peak,
        diode_1_conduction_fraction: integral[11],
        diode_2_conduction_fraction: integral[12],
        capacitor_energy_peak_j: cap_max,
        capacitor_energy_swing_j: cap_max - cap_min,
        input_energy_per_cycle_j: integral[5] * ctx.period,
        load_energy_per_cycle_j: load_power * ctx.period,
        fundamental_active_power_w: 0.5 * vmag2 * yr,
        fundamental_reactive_power_var: -0.5 * vmag2 * yi,
        input_current_thd: ((integral[4] - imag2 / 2.0).max(0.0) / (imag2 / 2.0)).sqrt(),
        energy_balance_error_w: balance,
        energy_balance_percent: balance_percent,
        input_admittance_real_s: yr,
        input_admittance_imag_s: yi,
        plug_only_admittance_real_s: plug_yr,
        plug_only_admittance_imag_s: plug_yi,
        equivalent_parallel_resistance_ohm: (yr.abs() > 1e-15).then_some(1.0 / yr),
        input_impedance_real_ohm: zr,
        input_impedance_imag_ohm: zi,
        input_voltage_fundamental_rms_v: (vmag2 / 2.0).sqrt(),
        input_current_fundamental_rms_a: (imag2 / 2.0).sqrt(),
        reflection_magnitude: reflection,
        quality_pass: balance_percent <= 1.0,
        calculation_checks: Vec::new(),
        resolution_check: None,
        capacitance_2_uf: circuit.capacitor_ratio * capacitance_uf,
        capacitor_esr_loss_w: esr_loss,
        capacitor_leakage_loss_w: leak_loss,
        reverse_recovery_loss_w: recovery_loss,
        source_coil_current_rms_a: integral[20].max(0.0).sqrt(),
        source_coil_energy_swing_j: coil_energy_max - coil_energy_min,
    };
    if !metrics.source_power_w.is_finite() || !metrics.energy_balance_percent.is_finite() {
        return Err("Nonfinite simulation result".into());
    }
    let mut point = ResultPoint { metrics, waveform };
    point.metrics.calculation_checks = crate::validation::check_point(circuit, &point);
    point.metrics.quality_pass &= point.metrics.calculation_checks.is_empty();
    Ok(point)
}

/// Parameters for a free-ringing tank probe at node A.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RingdownParams {
    pub tank_l_h: f64,
    pub tank_c_f: f64,
    /// Parallel loss conductance shunting the tank capacitor; negative values model
    /// an energy-pumping element (negative damping).
    pub loss_conductance_s: f64,
    pub initial_voltage_v: f64,
    pub ring_cycles: usize,
    /// When false the clamp diodes are disabled (open-circuit probe).
    pub clamp_enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RingdownResult {
    pub damped_frequency_hz: f64,
    /// Envelope decay constant of |va|; positive = decaying, negative = growing.
    pub decay_constant_per_s: f64,
    /// Effective parallel conductance implied by the decay: G = −2·C·α
    /// (a decaying envelope α<0 gives positive loss conductance).
    pub net_conductance_s: f64,
    pub negative_damping: bool,
    pub waveform: Vec<Trace>,
}

/// One free transient of the clamp node A with a parallel tank (L, C, G) attached.
/// Ringdown with the clamp diodes disabled is the open-circuit probe.
pub fn ringdown(circuit: &Circuit, params: RingdownParams) -> Result<RingdownResult, String> {
    if !params.tank_l_h.is_finite() || params.tank_l_h <= 0.0 {
        return Err("Tank inductance must be finite and positive".into());
    }
    if !params.tank_c_f.is_finite() || params.tank_c_f <= 0.0 {
        return Err("Tank capacitance must be finite and positive".into());
    }
    if !params.loss_conductance_s.is_finite() {
        return Err("Tank loss conductance must be finite".into());
    }
    if params.ring_cycles == 0 || params.ring_cycles > 2000 {
        return Err("Ring cycles must be between 1 and 2000".into());
    }
    if !params.initial_voltage_v.is_finite() {
        return Err("Initial voltage must be finite".into());
    }
    let f0 = (2.0 * PI * (params.tank_l_h * params.tank_c_f).sqrt()).recip();
    let mut probe = circuit.clone();
    // The clamp-node model needs a nonzero shunt capacitance; with both diodes off
    // an exactly-zero junction capacitance leaves the node system singular.
    probe.diode_capacitance_f = circuit.diode_capacitance_f.max(1e-9);
    if !params.clamp_enabled {
        probe.diode_resistance = 1e12;
        probe.diode_drop = 1e6;
    }
    let mode = Drive::Tank {
        l: params.tank_l_h,
        c: params.tank_c_f,
        g: params.loss_conductance_s,
    };
    let (layout, n) = layout_for(mode, &probe);
    let ctx = Context {
        circuit: &probe,
        plug1: circuit.load_capacitance,
        plug2: circuit.load_capacitance * circuit.capacitor_ratio,
        layout,
        mode,
        n,
        period: 1.0 / f0,
        steps: 4096,
        phase_offset: 0.0,
    };
    let mut state = [0.0; 6];
    if let Some(i) = layout.va {
        state[i] = params.initial_voltage_v;
    }
    let steps_per_cycle = 4096_usize;
    let total = params.ring_cycles * steps_per_cycle;
    let dt = (1.0 / f0) / steps_per_cycle as f64;
    let keep = total / 4096 + 1;
    let mut waveform = Vec::with_capacity(total / keep + 1);
    for k in 0..=total {
        if k % keep == 0 {
            let time = k as f64 * dt;
            waveform.push(make_trace(&ctx, time, state));
        }
        if k == total {
            break;
        }
        state = ctx.step(k as f64 * dt, state, dt)?;
        if !state.iter().all(|v| v.is_finite()) {
            return Err("Ringdown diverged; check loss conductance sign".into());
        }
    }
    // Envelope fit: local maxima of |va|.
    let peaks: Vec<(f64, f64)> = waveform
        .windows(3)
        .filter(|w| w[1].port_v.abs() > w[0].port_v.abs() && w[1].port_v.abs() >= w[2].port_v.abs())
        .map(|w| (w[1].time_s, w[1].port_v.abs()))
        .collect();
    if peaks.len() < 3 {
        return Err("Ringdown waveform too short to extract an envelope".into());
    }
    let mut freq_acc = 0.0;
    let mut freq_n = 0.0;
    for w in peaks.windows(2) {
        let (t0, _a0) = w[0];
        let (t1, _a1) = w[1];
        let dt0 = t1 - t0;
        if dt0 > 0.0 {
            freq_acc += 0.5 / dt0;
            freq_n += 1.0;
        }
    }
    // Endpoint-based decay: amplitude ratio across the whole record is far less
    // sensitive to per-peak switching noise than averaging adjacent log ratios.
    let (t_start, a_start) = peaks[peaks.len() / 4];
    let (t_end, a_end) = *peaks.last().unwrap();
    let (t_start, a_start) = (t_start, a_start.max(1e-30));
    let (t_end, a_end) = (t_end, a_end.max(1e-30));
    let decay = (a_end / a_start).abs().ln() / (t_end - t_start);
    if freq_n == 0.0 || t_end <= t_start {
        return Err("Ringdown envelope could not be fitted".into());
    }
    let fd = freq_acc / freq_n;
    let alpha = decay;
    let g_eff = -2.0 * params.tank_c_f * alpha;
    // Below ~0.01% of the tank's characteristic admittance the fitted value is
    // integration noise; decide the damping sign on a significant threshold.
    let characteristic_admittance_s = (params.tank_c_f / params.tank_l_h).sqrt();
    let noise_floor_s = 1e-4 * characteristic_admittance_s;
    Ok(RingdownResult {
        damped_frequency_hz: fd.abs(),
        decay_constant_per_s: alpha,
        net_conductance_s: g_eff,
        negative_damping: g_eff < -noise_floor_s,
        waveform,
    })
}

/// Open-circuit variant of the ringdown probe (clamp diodes disabled) so the tank
/// parameters can be identified without plug loading.
pub fn open_circuit_probe(
    circuit: &Circuit,
    params: RingdownParams,
) -> Result<RingdownResult, String> {
    ringdown(
        circuit,
        RingdownParams {
            clamp_enabled: false,
            ..params
        },
    )
}

fn make_trace(ctx: &Context<'_>, time: f64, s: State) -> Trace {
    let (_, z) = ctx.evaluate(time, s).ok().unwrap_or((
        [0.0; 6],
        Instant {
            vs: 0.0,
            va: 0.0,
            il: 0.0,
            input: 0.0,
            plug_input: 0.0,
            return_injection: 0.0,
            d1: 0.0,
            d2: 0.0,
            dx: 0.0,
            dy: 0.0,
            coil_a: 0.0,
        },
    ));
    let vl = ctx.layout.va.map_or(0.0, |i| s[0] - s[i]);
    Trace {
        time_s: time,
        source_v: z.vs,
        port_v: z.va,
        node_p_v: z.va - s[0],
        node_n_v: z.va - s[1],
        load_v: vl,
        load_a: z.il,
        input_a: z.input,
        plug_input_a: z.plug_input,
        return_injection_a: z.return_injection,
        return_path_power_w: 0.0,
        diode_1_a: z.d1,
        diode_2_a: z.d2,
        capacitor_1_a: ctx.circuit.load_capacitance * z.dx,
        capacitor_2_a: 0.0,
        load_capacitor_a: 0.0,
        input_power_w: z.vs * z.input,
        port_power_w: z.va * z.input,
        load_power_w: vl * z.il,
        capacitor_1_energy_j: 0.0,
        capacitor_2_energy_j: 0.0,
        load_capacitor_energy_j: 0.0,
        load_inductor_energy_j: ctx
            .layout
            .il
            .map_or(0.0, |i| 0.5 * ctx.circuit.inductance * s[i] * s[i]),
        source_coil_a: z.coil_a,
        source_coil_energy_j: ctx.layout.coil.map_or(0.0, |i| match ctx.mode {
            Drive::Tank { l, .. } => 0.5 * l * s[i] * s[i],
            _ => 0.5 * ctx.circuit.source_inductance * s[i] * s[i],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_keeps_state_and_time_when_controls_change() {
        let mut c = circuit();
        c.frequency = 1000.0;
        let mut run = Transient::new(c.clone(), 1.0).unwrap();
        assert_eq!(run.time_s(), 0.0);
        assert_eq!(run.waveform().len(), 1);
        run.advance(256).unwrap();
        let before = run.state;
        let time = run.time_s();
        let old_source = transient_context(&run.circuit, run.cap_uf, run.source_phase_offset)
            .evaluate(time, before)
            .unwrap()
            .1
            .vs;
        c.voltage_rms = 48.0;
        c.frequency = 500.0;
        c.resistance = 50.0;
        run.reconfigure(c, 2.0).unwrap();
        assert_eq!(run.time_s(), time);
        assert_eq!(run.state[0], before[0]);
        assert_eq!(run.state[1], before[1]);
        let new_source = transient_context(&run.circuit, run.cap_uf, run.source_phase_offset)
            .evaluate(time, run.state)
            .unwrap()
            .1
            .vs;
        assert!(
            (new_source / 2.0 - old_source).abs() < 1e-7,
            "source phase must not jump on frequency edit"
        );
        run.advance(128).unwrap();
        assert!(run.time_s() > time);
        assert!(run.waveform().windows(2).all(|w| w[1].time_s > w[0].time_s));
        assert_eq!(run.capacitance_uf(), 2.0);
    }

    #[test]
    fn transient_maps_inductors_and_dynamic_node_across_topology_changes() {
        let mut c = circuit();
        let mut run = Transient::new(c.clone(), 1.0).unwrap();
        run.advance(128).unwrap();
        let time = run.time_s();
        c.inductance = 1e-3;
        c.diode_capacitance_f = 1e-9;
        c.source_inductance = 1e-3;
        run.reconfigure(c, 1.0).unwrap();
        assert_eq!(run.time_s(), time);
        run.advance(64).unwrap();
        assert!(run.waveform().iter().all(|w| w.source_v.is_finite()));
        assert!(run.advance(513).is_err());
        assert!(Transient::new(circuit(), -1.0).is_err());
    }

    #[test]
    fn transient_chunking_advances_same_physical_state_without_periodic_reset() {
        let c = circuit();
        let mut together = Transient::new(c.clone(), 1.0).unwrap();
        let mut split = Transient::new(c, 1.0).unwrap();
        together.advance(256).unwrap();
        split.advance(128).unwrap();
        split.advance(128).unwrap();
        assert!((together.time_s - split.time_s).abs() < 1e-15);
        for (a, b) in together.state.iter().zip(split.state) {
            assert!((a - b).abs() < 1e-9);
        }
        assert!(!together.waveform().is_empty());
        assert_eq!(together.waveform().len(), split.waveform().len());
        assert!(together.waveform().len() <= 256);
        for (a, b) in together.waveform().iter().zip(split.waveform()) {
            assert_eq!(
                a.time_s, b.time_s,
                "scope sampling must not depend on chunk boundaries"
            );
        }
        assert!(
            together
                .waveform()
                .iter()
                .any(|sample| sample.capacitor_1_energy_j > 0.0)
        );
        assert_eq!(Transient::new(circuit(), 1.0).unwrap().time_s(), 0.0);
    }
    fn circuit() -> Circuit {
        Circuit {
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
        }
    }
    #[test]
    fn resistor_energy_and_rms() {
        let result = simulate(&circuit(), 10.0, Some(50.0)).unwrap();
        let m = result.metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
        assert!((m.load_power_w - m.load_voltage_rms_v.powi(2) / 100.0).abs() < 0.005);
        assert!(m.input_admittance_real_s > 0.0);
    }
    #[test]
    fn screenshot_case_converges() {
        let mut c = circuit();
        c.frequency = 10000.0;
        c.inductance = 0.0034;
        c.load_capacitance = 0.001;
        c.source_resistance = 0.9;
        let m = simulate(&c, 0.449843, Some(10.0)).unwrap().metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
    }
    #[test]
    fn nonlinear_loads_block_reverse_current_and_balance_threshold_power() {
        let mut c = circuit();
        c.voltage_rms = 36.0;
        c.frequency = 1000.0;
        c.resistance = 1.0;
        c.source_resistance = 0.5;
        c.load_model = LoadModel::Led { knee_voltage: 27.0 };
        let led = simulate(&c, 10.0, None).unwrap();
        assert!(led.waveform.iter().all(|w| w.load_a >= -1e-10));
        assert!(led.metrics.load_threshold_power_w >= 0.0);
        assert!(
            led.metrics.energy_balance_percent < 1.0,
            "{:?}",
            led.metrics
        );

        c.frequency = 50.0;
        c.resistance = 0.15;
        c.source_resistance = 2.0;
        c.load_model = LoadModel::Battery {
            open_circuit_voltage: 24.0,
        };
        let battery = simulate(&c, 100.0, None).unwrap();
        assert!(battery.waveform.iter().all(|w| w.load_a >= -1e-10));
        assert!(battery.metrics.load_threshold_power_w > 0.0);
        assert!(
            battery.metrics.energy_balance_percent < 1.0,
            "{:?}",
            battery.metrics
        );
    }
    #[test]
    fn bidirectional_dc_branch_does_not_imply_negative_ac_conductance() {
        let mut c = circuit();
        c.resistance = 5.0;
        c.load_model = LoadModel::BidirectionalBus { emf_voltage: 5.0 };
        let m = simulate(&c, 10.0, Some(50.0)).unwrap().metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
        // A bidirectional DC branch does not, by itself, establish negative AC conductance.
        assert!(m.input_admittance_real_s >= 0.0);
    }
    #[test]
    fn powered_ac_return_makes_negative_fundamental_conductance_with_energy_accounting() {
        let mut c = circuit();
        let passive = simulate(&c, 10.0, Some(50.0)).unwrap();
        assert!(passive.metrics.port_negative_power_fraction > 0.0);
        assert!(passive.metrics.port_power_w > 0.0);
        c.ac_return = Some(AcReturn {
            current_rms_a: 0.1,
            phase_deg: 0.0,
        });
        let result = simulate(&c, 10.0, Some(50.0)).unwrap();
        let m = result.metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
        assert!(m.input_admittance_real_s < 0.0, "{m:?}");
        assert!(m.fundamental_active_power_w < 0.0);
        assert!(m.source_power_w < 0.0);
        assert!(m.equivalent_parallel_resistance_ohm.unwrap() < 0.0);
        assert!(m.return_path_supplied_power_w > 0.0);
        assert!(m.plug_only_admittance_real_s > 0.0);
        assert!(m.port_negative_power_fraction > 0.0);
        assert!(
            (m.source_power_w + m.return_path_supplied_power_w - m.load_power_w - m.losses_w).abs()
                < 0.02
        );
        assert!(
            m.source_returned_energy_per_cycle_j
                > passive.metrics.source_returned_energy_per_cycle_j
        );
        assert!(result.waveform.iter().any(|w| w.input_power_w < 0.0));
    }

    #[test]
    fn gated_load_scales_delivered_power_and_keeps_balance() {
        let mut c = circuit();
        c.inductance = 0.0;
        c.pulse_gate = Some(PulseGate {
            repetition_hz: 50.0,
            duty: 0.5,
        });
        let gated = simulate(&c, 10.0, None).unwrap().metrics;
        c.pulse_gate = None;
        let full = simulate(&c, 10.0, None).unwrap().metrics;
        assert!(gated.energy_balance_percent < 1.0, "{gated:?}");
        let ratio = gated.load_energy_per_cycle_j / full.load_energy_per_cycle_j;
        // Held capacitor charge between gate pulses re-distributes load energy.
        assert!(ratio > 0.3 && ratio < 1.6, "gated/full ratio {ratio}");
    }

    #[test]
    fn leakage_and_esr_losses_show_up_and_stay_balanced() {
        let mut c = circuit();
        c.inductance = 0.0;
        c.capacitor_leakage_parallel_ohm = Some(1.0e5);
        c.capacitor_esr_ohm = 0.5;
        let m = simulate(&c, 10.0, None).unwrap().metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
        assert!(m.capacitor_leakage_loss_w > 0.0);
        assert!(m.capacitor_esr_loss_w >= 0.0);
    }

    #[test]
    fn dynamic_source_inductance_mode_preserves_energy_balance() {
        let mut c = circuit();
        c.voltage_rms = 24.0;
        c.inductance = 0.0;
        c.source_inductance = 0.05;
        c.diode_capacitance_f = 1e-8;
        let m = simulate(&c, 10.0, None).unwrap().metrics;
        assert!(m.energy_balance_percent < 1.0, "{m:?}");
        assert!(m.source_coil_current_rms_a > 0.0);
        assert!(m.source_coil_energy_swing_j > 0.0);
        assert_eq!(m.capacitance_2_uf, 10.0);
    }

    #[test]
    fn ringdown_matches_expectation_for_lossless_and_lossy_tank() {
        let c = circuit();
        for (g, expect_negative) in [(0.0, false), (1e-3, false), (-1e-3, true)] {
            let r = open_circuit_probe(
                &c,
                RingdownParams {
                    tank_l_h: 1e-3,
                    tank_c_f: 1e-6,
                    loss_conductance_s: g,
                    initial_voltage_v: 10.0,
                    ring_cycles: 8,
                    clamp_enabled: false,
                },
            )
            .unwrap();
            assert!((r.damped_frequency_hz - 5032.92).abs() < 5.0, "{r:?}");
            if g == 0.0 {
                assert!(r.decay_constant_per_s.abs() < 1.0, "{r:?}");
            } else {
                let alpha = (g / (2.0e-6)).abs();
                assert!(
                    (r.decay_constant_per_s.abs() - alpha).abs() < 0.1 * alpha,
                    "{r:?}"
                );
            }
            assert_eq!(r.negative_damping, expect_negative);
        }
    }
}
