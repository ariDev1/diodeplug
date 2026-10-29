use serde::Serialize;
use std::f64::consts::PI;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct Measurement {
    pub samples: usize,
    pub periods: usize,
    pub voltage_fundamental_rms_v: f64,
    pub current_fundamental_rms_a: f64,
    pub conductance_s: f64,
    pub susceptance_s: f64,
    pub parallel_resistance_ohm: Option<f64>,
    pub fundamental_active_power_w: f64,
    pub fundamental_reactive_power_var: f64,
    pub total_average_power_w: f64,
    pub negative_power_fraction: f64,
    pub returned_energy_per_cycle_j: f64,
}

/// Current must be positive into the measured A–B port. Samples include both
/// endpoints of an integer-period window; trapezoidal quadrature handles nonuniform time.
pub fn analyze(samples: &[(f64, f64, f64)], frequency_hz: f64) -> Result<Measurement, String> {
    if !frequency_hz.is_finite() || frequency_hz <= 0.0 || samples.len() < 21 {
        return Err("Frequency must be positive and at least 21 samples are required".into());
    }
    if samples
        .iter()
        .any(|s| !s.0.is_finite() || !s.1.is_finite() || !s.2.is_finite())
    {
        return Err("All time, voltage and current samples must be finite".into());
    }
    let span = samples.last().unwrap().0 - samples[0].0;
    let periods_float = span * frequency_hz;
    let periods = periods_float.round() as usize;
    if span <= 0.0 || periods < 1 || (periods_float - periods as f64).abs() > 0.01 {
        return Err(
            "Measurement must span an integer number of full source periods (within 1%)".into(),
        );
    }
    if (samples.len() - 1) / periods < 20 {
        return Err("Provide at least 20 sample intervals per source period".into());
    }
    let (mut vr, mut vi, mut ir, mut ii) = (0.0, 0.0, 0.0, 0.0);
    let (mut power, mut negative_duration, mut returned_energy) = (0.0, 0.0, 0.0);
    let omega = 2.0 * PI * frequency_hz;
    for pair in samples.windows(2) {
        let (t0, v0, i0) = pair[0];
        let (t1, v1, i1) = pair[1];
        let dt = t1 - t0;
        if dt <= 0.0 {
            return Err("Sample times must be strictly increasing".into());
        }
        let (c0, s0) = ((omega * t0).cos(), (omega * t0).sin());
        let (c1, s1) = ((omega * t1).cos(), (omega * t1).sin());
        let factor = dt / span;
        vr += factor * (v0 * c0 + v1 * c1);
        vi -= factor * (v0 * s0 + v1 * s1);
        ir += factor * (i0 * c0 + i1 * c1);
        ii -= factor * (i0 * s0 + i1 * s1);
        let p0 = v0 * i0;
        let p1 = v1 * i1;
        power += dt * (p0 + p1) * 0.5 / span;
        // Linearly interpolated crossing avoids treating an entire edge as negative.
        if p0 < 0.0 && p1 < 0.0 {
            negative_duration += dt;
            returned_energy += -dt * (p0 + p1) * 0.5;
        } else if (p0 < 0.0) != (p1 < 0.0) {
            let part = p0.abs() / (p0.abs() + p1.abs());
            let duration = if p0 < 0.0 {
                dt * part
            } else {
                dt * (1.0 - part)
            };
            negative_duration += duration;
            returned_energy += 0.5 * duration * (if p0 < 0.0 { -p0 } else { -p1 });
        }
    }
    let vm2 = vr * vr + vi * vi;
    if vm2 <= 1e-20 {
        return Err("Port fundamental voltage is too small to extract admittance".into());
    }
    let g = (ir * vr + ii * vi) / vm2;
    let b = (ii * vr - ir * vi) / vm2;
    Ok(Measurement {
        samples: samples.len(),
        periods,
        voltage_fundamental_rms_v: (vm2 * 0.5).sqrt(),
        current_fundamental_rms_a: ((ir * ir + ii * ii) * 0.5).sqrt(),
        conductance_s: g,
        susceptance_s: b,
        parallel_resistance_ohm: if g.abs() > 1e-15 { Some(1.0 / g) } else { None },
        fundamental_active_power_w: 0.5 * vm2 * g,
        fundamental_reactive_power_var: -0.5 * vm2 * b,
        total_average_power_w: power,
        negative_power_fraction: negative_duration / span,
        returned_energy_per_cycle_j: returned_energy / periods as f64,
    })
}

/// Accepts `time_s,port_V,input_A` (simulator export) or
/// `time_s,voltage_V,current_A`; quoted header names are allowed.
pub fn import_csv(
    path: &Path,
    frequency_hz: f64,
    invert_current: bool,
) -> Result<Measurement, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("CSV is empty")?;
    let names: Vec<_> = header
        .split(',')
        .map(|n| n.trim().trim_matches('"').to_ascii_lowercase())
        .collect();
    let column = |options: &[&str]| -> Result<usize, String> {
        names
            .iter()
            .position(|n| options.iter().any(|opt| n == opt))
            .ok_or(format!(
                "CSV needs one of these columns: {}",
                options.join(" / ")
            ))
    };
    let ti = column(&["time_s"])?;
    let vi = column(&["port_v", "voltage_v"])?;
    let ii = column(&["input_a", "input_total_a", "current_a"])?;
    let mut samples = Vec::new();
    for (row, line) in lines.enumerate() {
        let values: Vec<_> = line.split(',').map(str::trim).collect();
        let read = |index: usize| -> Result<f64, String> {
            values
                .get(index)
                .ok_or(format!("Missing column in row {}", row + 2))?
                .parse::<f64>()
                .map_err(|_| format!("Invalid number in row {}", row + 2))
        };
        samples.push((
            read(ti)?,
            read(vi)?,
            read(ii)? * if invert_current { -1.0 } else { 1.0 },
        ));
    }
    analyze(&samples, frequency_hz)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sine_current_return_has_negative_g_and_negative_total_power() {
        let samples: Vec<_> = (0..=400)
            .map(|n| {
                let t = n as f64 / 400.0 * 0.02;
                let v = 10.0_f64.sqrt() * 2.0 * (2.0 * PI * 50.0 * t).sin();
                (t, v, -v / 50.0)
            })
            .collect();
        let m = analyze(&samples, 50.0).unwrap();
        assert!((m.conductance_s + 0.02).abs() < 1e-7, "{m:?}");
        assert!(m.fundamental_active_power_w < 0.0);
        assert!(m.total_average_power_w < 0.0);
        assert!((m.negative_power_fraction - 1.0).abs() < 0.01);
        assert!((m.parallel_resistance_ohm.unwrap() + 50.0).abs() < 1e-5);
        assert!(analyze(&samples, 60.0).is_err());
    }
    #[test]
    fn exported_waveform_columns_and_probe_direction_are_supported() {
        let path =
            std::env::temp_dir().join(format!("diode-plug-measure-{}.csv", std::process::id()));
        let mut csv = String::from("time_s,port_V,input_total_A\n");
        for n in 0..=200 {
            let t = n as f64 * 0.02 / 200.0;
            let v = 10.0 * (2.0 * PI * 50.0 * t).sin();
            csv.push_str(&format!("{t},{v},{}\n", -v / 100.0));
        }
        std::fs::write(&path, csv).unwrap();
        let negative = import_csv(&path, 50.0, false).unwrap();
        let positive = import_csv(&path, 50.0, true).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!((negative.conductance_s + 0.01).abs() < 1e-8);
        assert!((positive.conductance_s - 0.01).abs() < 1e-8);
    }
}
