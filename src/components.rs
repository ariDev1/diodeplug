//! Typical diode data for the piecewise-linear solver.
//!
//! Each entry is a convenience starting point derived from datasheet tables and
//! curves. `vf0_v` is the extrapolated zero-current knee and `rd_ohm` is the
//! slope resistance used by [`crate::solver::Circuit`]. Values are typical,
//! not manufacturer-guaranteed limits; verify the exact part before treating a
//! simulation as hardware validation.

/// A named diode model for the advanced datasheet entry section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiodeModel {
    pub name: &'static str,
    /// Extrapolated zero-current forward voltage.
    pub vf0_v: f64,
    /// Forward slope resistance.
    pub rd_ohm: f64,
    /// Typical junction capacitance at the datasheet test voltage (often a few
    /// volts reverse bias; some high-voltage parts specify it at a much higher
    /// voltage).
    pub cj_f: f64,
    /// Typical reverse-recovery charge per turn-off event.
    pub qrr_nc: f64,
    /// Maximum repetitive reverse voltage rating.
    pub v_rrm_v: f64,
    /// Maximum average forward current rating.
    pub i_f_avg_a: f64,
}

impl DiodeModel {
    /// Compare the solver inputs with this model, ignoring harmless formatting
    /// differences in the displayed values.
    pub fn matches(&self, vf0_v: f64, rd_ohm: f64, cj_f: f64, qrr_nc: f64) -> bool {
        approx(self.vf0_v, vf0_v)
            && approx(self.rd_ohm, rd_ohm)
            && approx(self.cj_f, cj_f)
            && approx(self.qrr_nc, qrr_nc)
    }
}

fn approx(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs()).max(1e-30);
    (a - b).abs() <= 1e-9 * scale
}

/// Common rectifiers, fast-recovery diodes and Schottkys used in mains,
/// low-voltage and switched-mode diode-plug experiments.
pub const DIODE_MODELS: &[DiodeModel] = &[
    DiodeModel {
        name: "1N4148",
        vf0_v: 0.65,
        rd_ohm: 0.80,
        cj_f: 4e-12,
        qrr_nc: 4.0,
        v_rrm_v: 100.0,
        i_f_avg_a: 0.2,
    },
    DiodeModel {
        name: "1N4007",
        vf0_v: 0.78,
        rd_ohm: 0.10,
        cj_f: 15e-12,
        qrr_nc: 2000.0,
        v_rrm_v: 1000.0,
        i_f_avg_a: 1.0,
    },
    DiodeModel {
        name: "1N5408",
        vf0_v: 0.85,
        rd_ohm: 0.10,
        cj_f: 30e-12,
        qrr_nc: 5000.0,
        v_rrm_v: 1000.0,
        i_f_avg_a: 3.0,
    },
    DiodeModel {
        name: "1N4937",
        vf0_v: 1.00,
        rd_ohm: 0.30,
        cj_f: 15e-12,
        qrr_nc: 150.0,
        v_rrm_v: 600.0,
        i_f_avg_a: 1.0,
    },
    DiodeModel {
        name: "UF4007",
        vf0_v: 0.90,
        rd_ohm: 0.40,
        cj_f: 15e-12,
        qrr_nc: 30.0,
        v_rrm_v: 1000.0,
        i_f_avg_a: 1.0,
    },
    DiodeModel {
        name: "MUR460",
        vf0_v: 1.00,
        rd_ohm: 0.10,
        cj_f: 50e-12,
        qrr_nc: 100.0,
        v_rrm_v: 600.0,
        i_f_avg_a: 4.0,
    },
    // ON Semiconductor MUR480E/D Rev. 9 (MUR4100EG). The 25 °C curve fit
    // reproduces the typical 3 A / 4 A forward points; Qrr is estimated from
    // trr and the typical peak recovery current.
    DiodeModel {
        name: "MUR4100E",
        vf0_v: 1.00,
        rd_ohm: 0.10,
        cj_f: 20e-12,
        qrr_nc: 100.0,
        v_rrm_v: 1000.0,
        i_f_avg_a: 4.0,
    },
    // Vishay VS-HFA16PB120-N3, document 94057 Rev. 29-Nov-2019. Vf0/Rd fit
    // the typical 16 A and 32 A forward points; Cj is the tabulated 27 pF at
    // VR = 200 V, and Qrr is the typical 260 nC at 16 A and 200 A/µs.
    DiodeModel {
        name: "VS-HFA16PB120-N3",
        vf0_v: 1.80,
        rd_ohm: 0.04375,
        cj_f: 27e-12,
        qrr_nc: 260.0,
        v_rrm_v: 1200.0,
        i_f_avg_a: 16.0,
    },
    DiodeModel {
        name: "1N5819",
        vf0_v: 0.38,
        rd_ohm: 0.17,
        cj_f: 120e-12,
        qrr_nc: 0.0,
        v_rrm_v: 40.0,
        i_f_avg_a: 1.0,
    },
    DiodeModel {
        name: "1N5822",
        vf0_v: 0.40,
        rd_ohm: 0.10,
        cj_f: 300e-12,
        qrr_nc: 0.0,
        v_rrm_v: 40.0,
        i_f_avg_a: 3.0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_diode_library_is_unique_and_plausible() {
        assert!(DIODE_MODELS.len() >= 10);
        for (index, model) in DIODE_MODELS.iter().enumerate() {
            assert!(!model.name.is_empty());
            assert!(model.vf0_v >= 0.0 && model.vf0_v.is_finite());
            assert!(model.rd_ohm > 0.0 && model.rd_ohm.is_finite());
            assert!(model.cj_f >= 0.0 && model.cj_f.is_finite());
            assert!(model.qrr_nc >= 0.0 && model.qrr_nc.is_finite());
            assert!(model.v_rrm_v > 0.0 && model.v_rrm_v.is_finite());
            assert!(model.i_f_avg_a > 0.0 && model.i_f_avg_a.is_finite());
            assert!(
                DIODE_MODELS[index + 1..]
                    .iter()
                    .all(|other| other.name != model.name)
            );
        }
    }

    #[test]
    fn known_parts_match_expected_fit_values() {
        let part = |name| {
            DIODE_MODELS
                .iter()
                .find(|model| model.name == name)
                .unwrap()
        };
        let d = part("1N4007");
        assert!((d.vf0_v - 0.78).abs() < 1e-12);
        assert!((d.rd_ohm - 0.10).abs() < 1e-12);
        assert_eq!(d.v_rrm_v, 1000.0);
        assert_eq!(d.i_f_avg_a, 1.0);

        let schottky = part("1N5819");
        assert_eq!(schottky.qrr_nc, 0.0);
        assert!(schottky.vf0_v < 0.5);
        assert_eq!(schottky.v_rrm_v, 40.0);

        // ON Semiconductor MUR4100EG: typical 25 °C curve near 3–4 A.
        let mur = part("MUR4100E");
        assert!((mur.vf0_v + 4.0 * mur.rd_ohm - 1.40).abs() < 0.02);
        assert_eq!(mur.v_rrm_v, 1000.0);
        assert_eq!(mur.i_f_avg_a, 4.0);
        assert!((mur.qrr_nc - 100.0).abs() < 1e-12);

        // Vishay VS-HFA16PB120-N3: typical 16 A and 32 A forward points.
        let hfa = part("VS-HFA16PB120-N3");
        assert!((hfa.vf0_v + 16.0 * hfa.rd_ohm - 2.5).abs() < 0.01);
        assert!((hfa.vf0_v + 32.0 * hfa.rd_ohm - 3.2).abs() < 0.01);
        assert_eq!(hfa.v_rrm_v, 1200.0);
        assert_eq!(hfa.i_f_avg_a, 16.0);
        assert!((hfa.qrr_nc - 260.0).abs() < 1e-12);
        assert!((hfa.cj_f - 27e-12).abs() < 1e-24);
    }
}
