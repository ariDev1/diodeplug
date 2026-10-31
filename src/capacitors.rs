//! Typical capacitor presets for the diode-plug load model.
//!
//! The solver stores one capacitance, one series ESR and one parallel leakage
//! resistance per capacitor. Real film capacitors are frequency-dependent, so
//! ESR here is calculated from the datasheet dissipation factor at the stated
//! test frequency: ESR = tanδ / (2π f C). Recalculate it when your operating
//! frequency differs. Values are typical starting points, not guarantees.

/// A named capacitor preset for the advanced component section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapacitorModel {
    pub name: &'static str,
    /// Group shown in the preset picker.
    pub group: &'static str,
    pub capacitance_f: f64,
    pub rated_voltage_v: f64,
    /// True for an AC RMS rating; false for a DC rating.
    pub rated_ac: bool,
    /// Datasheet dissipation factor at `esr_frequency_hz`.
    pub tan_delta: f64,
    pub esr_frequency_hz: f64,
    /// Insulation resistance used as the parallel leakage path.
    pub leakage_parallel_ohm: Option<f64>,
}

impl CapacitorModel {
    /// Series ESR implied by the datasheet dissipation factor.
    pub fn esr_ohm(&self) -> f64 {
        self.tan_delta / (std::f64::consts::TAU * self.esr_frequency_hz * self.capacitance_f)
    }

    /// Compare the solver inputs with this preset, ignoring harmless formatting
    /// differences in the displayed values.
    pub fn matches(&self, capacitance_f: f64, esr_ohm: f64, leakage: Option<f64>) -> bool {
        approx(self.capacitance_f, capacitance_f)
            && approx(self.esr_ohm(), esr_ohm)
            && match (self.leakage_parallel_ohm, leakage) {
                (None, None) => true,
                (Some(a), Some(b)) => approx(a, b),
                _ => false,
            }
    }
}

fn approx(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs()).max(1e-30);
    (a - b).abs() <= 1e-9 * scale
}

/// Motor-run capacitors and common WIMA pulse/general-purpose film capacitors.
/// Motor-run ESR is evaluated at 50 Hz; WIMA ESR uses the highest frequency at
/// which the datasheet gives tanδ for that capacitance.
pub const CAPACITOR_MODELS: &[CapacitorModel] = &[
    // CBB65 metallized-polypropylene motor-run capacitors, 450 VAC, 50/60 Hz.
    // ESR estimated from tanδ <= 0.002 at 50 Hz; IR >= 3 GΩ.
    CapacitorModel {
        name: "Motor-run 4 µF / 450 VAC",
        group: "Motor run",
        capacitance_f: 4e-6,
        rated_voltage_v: 450.0,
        rated_ac: true,
        tan_delta: 0.002,
        esr_frequency_hz: 50.0,
        leakage_parallel_ohm: Some(3e9),
    },
    CapacitorModel {
        name: "Motor-run 8 µF / 450 VAC",
        group: "Motor run",
        capacitance_f: 8e-6,
        rated_voltage_v: 450.0,
        rated_ac: true,
        tan_delta: 0.002,
        esr_frequency_hz: 50.0,
        leakage_parallel_ohm: Some(3e9),
    },
    CapacitorModel {
        name: "Motor-run 10 µF / 450 VAC",
        group: "Motor run",
        capacitance_f: 10e-6,
        rated_voltage_v: 450.0,
        rated_ac: true,
        tan_delta: 0.002,
        esr_frequency_hz: 50.0,
        leakage_parallel_ohm: Some(3e9),
    },
    CapacitorModel {
        name: "Motor-run 16 µF / 450 VAC",
        group: "Motor run",
        capacitance_f: 16e-6,
        rated_voltage_v: 450.0,
        rated_ac: true,
        tan_delta: 0.002,
        esr_frequency_hz: 50.0,
        leakage_parallel_ohm: Some(3e9),
    },
    CapacitorModel {
        name: "Motor-run 25 µF / 450 VAC",
        group: "Motor run",
        capacitance_f: 25e-6,
        rated_voltage_v: 450.0,
        rated_ac: true,
        tan_delta: 0.002,
        esr_frequency_hz: 50.0,
        leakage_parallel_ohm: Some(3e9),
    },
    CapacitorModel {
        name: "Motor-run 40 µF / 450 VAC",
        group: "Motor run",
        capacitance_f: 40e-6,
        rated_voltage_v: 450.0,
        rated_ac: true,
        tan_delta: 0.002,
        esr_frequency_hz: 50.0,
        leakage_parallel_ohm: Some(3e9),
    },
    // WIMA MKP 10: metallized PP pulse capacitor, 630 VDC / 400 VAC.
    CapacitorModel {
        name: "WIMA MKP 10 · 0.1 µF / 630 VDC",
        group: "WIMA MKP 10",
        capacitance_f: 0.1e-6,
        rated_voltage_v: 630.0,
        rated_ac: false,
        tan_delta: 15e-4,
        esr_frequency_hz: 100e3,
        leakage_parallel_ohm: Some(1e11),
    },
    CapacitorModel {
        name: "WIMA MKP 10 · 1 µF / 630 VDC",
        group: "WIMA MKP 10",
        capacitance_f: 1e-6,
        rated_voltage_v: 630.0,
        rated_ac: false,
        tan_delta: 6e-4,
        esr_frequency_hz: 10e3,
        leakage_parallel_ohm: Some(3e10),
    },
    CapacitorModel {
        name: "WIMA MKP 10 · 4.7 µF / 630 VDC",
        group: "WIMA MKP 10",
        capacitance_f: 4.7e-6,
        rated_voltage_v: 630.0,
        rated_ac: false,
        tan_delta: 6e-4,
        esr_frequency_hz: 1e3,
        leakage_parallel_ohm: Some(6.382_978_7e9),
    },
    // WIMA MKP 4: metallized PP general-purpose capacitor, 400 VDC / 220 VAC.
    CapacitorModel {
        name: "WIMA MKP 4 · 1 µF / 400 VDC",
        group: "WIMA MKP 4",
        capacitance_f: 1e-6,
        rated_voltage_v: 400.0,
        rated_ac: false,
        tan_delta: 8e-4,
        esr_frequency_hz: 10e3,
        leakage_parallel_ohm: Some(3e10),
    },
    CapacitorModel {
        name: "WIMA MKP 4 · 4.7 µF / 400 VDC",
        group: "WIMA MKP 4",
        capacitance_f: 4.7e-6,
        rated_voltage_v: 400.0,
        rated_ac: false,
        tan_delta: 6e-4,
        esr_frequency_hz: 1e3,
        leakage_parallel_ohm: Some(6.382_978_7e9),
    },
    CapacitorModel {
        name: "WIMA MKP 4 · 10 µF / 400 VDC",
        group: "WIMA MKP 4",
        capacitance_f: 10e-6,
        rated_voltage_v: 400.0,
        rated_ac: false,
        tan_delta: 6e-4,
        esr_frequency_hz: 1e3,
        leakage_parallel_ohm: Some(3e9),
    },
    // WIMA FKP 1: foil-electrode PP pulse capacitor, 630 VDC / 400 VAC.
    CapacitorModel {
        name: "WIMA FKP 1 · 0.01 µF / 630 VDC",
        group: "WIMA FKP 1",
        capacitance_f: 0.01e-6,
        rated_voltage_v: 630.0,
        rated_ac: false,
        tan_delta: 10e-4,
        esr_frequency_hz: 100e3,
        leakage_parallel_ohm: Some(1e11),
    },
    CapacitorModel {
        name: "WIMA FKP 1 · 0.1 µF / 630 VDC",
        group: "WIMA FKP 1",
        capacitance_f: 0.1e-6,
        rated_voltage_v: 630.0,
        rated_ac: false,
        tan_delta: 10e-4,
        esr_frequency_hz: 100e3,
        leakage_parallel_ohm: Some(1e11),
    },
    CapacitorModel {
        name: "WIMA FKP 1 · 0.47 µF / 630 VDC",
        group: "WIMA FKP 1",
        capacitance_f: 0.47e-6,
        rated_voltage_v: 630.0,
        rated_ac: false,
        tan_delta: 6e-4,
        esr_frequency_hz: 10e3,
        leakage_parallel_ohm: Some(6.382_978_7e10),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacitor_library_is_unique_and_plausible() {
        assert!(CAPACITOR_MODELS.len() >= 12);
        for (index, model) in CAPACITOR_MODELS.iter().enumerate() {
            assert!(!model.name.is_empty());
            assert!(!model.group.is_empty());
            assert!(model.capacitance_f > 0.0 && model.capacitance_f.is_finite());
            assert!(model.rated_voltage_v > 0.0 && model.rated_voltage_v.is_finite());
            assert!(model.tan_delta > 0.0 && model.tan_delta < 0.01);
            assert!(model.esr_frequency_hz > 0.0 && model.esr_frequency_hz.is_finite());
            assert!(model.esr_ohm() > 0.0 && model.esr_ohm().is_finite());
            if let Some(leakage) = model.leakage_parallel_ohm {
                assert!(leakage > 0.0 && leakage.is_finite());
            }
            assert!(
                CAPACITOR_MODELS[index + 1..]
                    .iter()
                    .all(|other| other.name != model.name)
            );
        }
    }

    #[test]
    fn known_capacitors_match_datasheet_fits() {
        let part = |name| {
            CAPACITOR_MODELS
                .iter()
                .find(|model| model.name == name)
                .unwrap()
        };

        let motor = part("Motor-run 10 µF / 450 VAC");
        assert!((motor.capacitance_f - 10e-6).abs() < 1e-15);
        assert!((motor.esr_ohm() - std::f64::consts::FRAC_2_PI).abs() < 1e-8);
        assert_eq!(motor.rated_voltage_v, 450.0);
        assert!(motor.rated_ac);
        assert_eq!(motor.leakage_parallel_ohm, Some(3e9));

        let mkp10 = part("WIMA MKP 10 · 1 µF / 630 VDC");
        assert!((mkp10.esr_ohm() - 0.009_549_3).abs() < 1e-8);
        assert_eq!(mkp10.rated_voltage_v, 630.0);
        assert_eq!(mkp10.leakage_parallel_ohm, Some(3e10));

        let fkp1 = part("WIMA FKP 1 · 0.01 µF / 630 VDC");
        assert!((fkp1.esr_ohm() - 0.159_154_94).abs() < 1e-8);
        assert_eq!(fkp1.leakage_parallel_ohm, Some(1e11));
    }
}
