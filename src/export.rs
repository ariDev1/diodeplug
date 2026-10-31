use crate::sweep::Sweep;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

const HEADER_LEN: usize = 49;

fn resolution_stages(check: &crate::validation::ResolutionCheck) -> String {
    let mut text = Vec::new();
    let mut stage = Some(check);
    while let Some(level) = stage {
        text.push(format!(
            "{}->{}: max {}%; balance {}%",
            level.base_steps,
            level.refined_steps,
            level
                .max_difference_percent
                .map_or("n/a".into(), |v| format!("{v:.4}")),
            level
                .refined_balance_percent
                .map_or("n/a".into(), |v| format!("{v:.4}"))
        ));
        stage = level.follow_up.as_deref();
    }
    text.join(" | ")
}

pub fn write(sweep: &Sweep, folder: &Path) -> Result<(), String> {
    if folder.exists()
        && (!folder.is_dir()
            || folder
                .read_dir()
                .map_err(|e| e.to_string())?
                .next()
                .is_some())
    {
        return Err(format!(
            "Output directory {} is not empty",
            folder.display()
        ));
    }
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let json =
        BufWriter::new(File::create(folder.join("results.json")).map_err(|e| e.to_string())?);
    serde_json::to_writer_pretty(json, sweep).map_err(|e| e.to_string())?;
    let mut table =
        BufWriter::new(File::create(folder.join("sweep.csv")).map_err(|e| e.to_string())?);
    writeln!(table, "C_each_uF,load_V_rms,load_total_W,load_resistor_W,load_threshold_W,load_branch_A_mean,input_A_rms,Zin_real_ohm,Zin_imag_ohm,G_total_S,B_total_S,reflection,source_W,port_W,diode_loss_W,source_R_loss_W,cap_esr_loss_W,cap_leakage_loss_W,reverse_recovery_loss_W,cap_energy_swing_J,diode_1_peak_A,diode_2_peak_A,fundamental_P_W,fundamental_Q_var,input_THD,balance_percent,quality_pass,convergence,cycles,G_plug_only_S,B_plug_only_S,R_parallel_ohm,AC_return_W_absorbed,AC_return_W_supplied,port_negative_fraction,port_return_J_per_cycle,source_return_J_per_cycle,source_coil_A_rms,source_coil_energy_swing_J,calculation_checks,resolution_base_steps,resolution_refined_steps,resolution_max_delta_percent,resolution_refined_balance_percent,resolution_initial_issues,resolution_stages,resolution_accepted_steps,resolution_trend,error")
        .map_err(|e| e.to_string())?;
    for (i, point) in sweep.results.iter().enumerate() {
        match point {
            Ok(row) => {
                let m = &row.metrics;
                let cells = [
                    m.capacitance_each_uf.to_string(),
                    m.load_voltage_rms_v.to_string(),
                    m.load_power_w.to_string(),
                    m.load_resistor_power_w.to_string(),
                    m.load_threshold_power_w.to_string(),
                    m.load_branch_current_mean_a.to_string(),
                    m.input_current_rms_a.to_string(),
                    m.input_impedance_real_ohm.to_string(),
                    m.input_impedance_imag_ohm.to_string(),
                    m.input_admittance_real_s.to_string(),
                    m.input_admittance_imag_s.to_string(),
                    m.reflection_magnitude
                        .map_or(String::new(), |v| v.to_string()),
                    m.source_power_w.to_string(),
                    m.port_power_w.to_string(),
                    m.diode_loss_w.to_string(),
                    m.source_resistor_loss_w.to_string(),
                    m.capacitor_esr_loss_w.to_string(),
                    m.capacitor_leakage_loss_w.to_string(),
                    m.reverse_recovery_loss_w.to_string(),
                    m.capacitor_energy_swing_j.to_string(),
                    m.diode_1_peak_a.to_string(),
                    m.diode_2_peak_a.to_string(),
                    m.fundamental_active_power_w.to_string(),
                    m.fundamental_reactive_power_var.to_string(),
                    m.input_current_thd.to_string(),
                    m.energy_balance_percent.to_string(),
                    m.quality_pass.to_string(),
                    m.convergence_method.clone(),
                    m.cycles_integrated.to_string(),
                    m.plug_only_admittance_real_s.to_string(),
                    m.plug_only_admittance_imag_s.to_string(),
                    m.equivalent_parallel_resistance_ohm
                        .map_or(String::new(), |r| r.to_string()),
                    m.return_path_absorbed_power_w.to_string(),
                    m.return_path_supplied_power_w.to_string(),
                    m.port_negative_power_fraction.to_string(),
                    m.port_returned_energy_per_cycle_j.to_string(),
                    m.source_returned_energy_per_cycle_j.to_string(),
                    m.source_coil_current_rms_a.to_string(),
                    m.source_coil_energy_swing_j.to_string(),
                    format!(
                        "\"{}\"",
                        m.calculation_checks.join("; ").replace('"', "\"\"")
                    ),
                    m.resolution_check
                        .as_ref()
                        .map_or(String::new(), |r| r.base_steps.to_string()),
                    m.resolution_check
                        .as_ref()
                        .map_or(String::new(), |r| r.refined_steps.to_string()),
                    m.resolution_check
                        .as_ref()
                        .and_then(|r| r.max_difference_percent)
                        .map_or(String::new(), |v| v.to_string()),
                    m.resolution_check
                        .as_ref()
                        .and_then(|r| r.refined_balance_percent)
                        .map_or(String::new(), |v| v.to_string()),
                    format!(
                        "\"{}\"",
                        m.resolution_check
                            .as_ref()
                            .map_or(String::new(), |r| r.issues.join("; "))
                            .replace('"', "\"\"")
                    ),
                    format!(
                        "\"{}\"",
                        m.resolution_check
                            .as_ref()
                            .map_or(String::new(), resolution_stages)
                            .replace('"', "\"\"")
                    ),
                    m.resolution_check
                        .as_ref()
                        .and_then(|r| r.accepted_steps)
                        .map_or(String::new(), |v| v.to_string()),
                    format!(
                        "\"{}\"",
                        m.resolution_check
                            .as_ref()
                            .and_then(|r| r.trend.as_deref())
                            .unwrap_or("")
                            .replace('"', "\"\"")
                    ),
                    String::new(),
                ];
                writeln!(table, "{}", cells.join(",")).map_err(|e| e.to_string())?;
            }
            Err(error) => {
                let clean = error.replace('"', "\"\"");
                let mut cells = vec![String::new(); HEADER_LEN];
                cells[HEADER_LEN - 1] = format!("\"point {i}: {clean}\"");
                writeln!(table, "{}", cells.join(",")).map_err(|e| e.to_string())?;
            }
        }
    }
    let waveform_index = sweep
        .observation_comparison
        .as_ref()
        .and_then(|c| {
            sweep.results.iter().position(|r| {
                r.as_ref().is_ok_and(|p| {
                    p.metrics.capacitance_each_uf == c.simulation_capacitance_each_uf
                })
            })
        })
        .or(sweep.recommended_index)
        .or_else(|| sweep.results.iter().position(Result::is_ok));
    if let Some(point) = waveform_index.and_then(|i| sweep.results[i].as_ref().ok()) {
        let mut wave =
            BufWriter::new(File::create(folder.join("waveform.csv")).map_err(|e| e.to_string())?);
        writeln!(
            wave,
            "time_s,source_V,port_V,node_P_V,node_N_V,load_V,load_A,input_total_A,diode_1_A,diode_2_A,capacitor_1_A,capacitor_2_A,load_capacitor_A,input_power_W,port_power_W,load_power_W,capacitor_1_energy_J,capacitor_2_energy_J,load_capacitor_energy_J,load_inductor_energy_J,source_coil_A,source_coil_energy_J,plug_only_input_A,powered_return_injection_A,powered_return_path_power_W"
        )
        .map_err(|e| e.to_string())?;
        for w in &point.waveform {
            let cells = [
                w.time_s,
                w.source_v,
                w.port_v,
                w.node_p_v,
                w.node_n_v,
                w.load_v,
                w.load_a,
                w.input_a,
                w.diode_1_a,
                w.diode_2_a,
                w.capacitor_1_a,
                w.capacitor_2_a,
                w.load_capacitor_a,
                w.input_power_w,
                w.port_power_w,
                w.load_power_w,
                w.capacitor_1_energy_j,
                w.capacitor_2_energy_j,
                w.load_capacitor_energy_j,
                w.load_inductor_energy_j,
                w.source_coil_a,
                w.source_coil_energy_j,
                w.plug_input_a,
                w.return_injection_a,
                w.return_path_power_w,
            ];
            writeln!(wave, "{}", cells.map(|x| x.to_string()).join(","))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinement_columns_and_failed_rows_have_the_same_width() {
        let mut config = crate::presets::all()[9].config.clone();
        config.points = 2;
        let mut sweep = crate::sweep::run(config, |_, _| {}).unwrap();
        sweep.results[1] = Err("intentional failure".into());
        sweep.recommended_index = None;
        let temp_root = Path::new("/tmp/opencode");
        let temp_root = if temp_root.is_dir() {
            temp_root.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        let folder = temp_root.join(format!(
            "diode-plug-export-test-{}-{}",
            std::process::id(),
            sweep.recorded_unix_ms
        ));
        write(&sweep, &folder).unwrap();
        let csv = fs::read_to_string(folder.join("sweep.csv")).unwrap();
        let rows: Vec<_> = csv.lines().collect();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.split(',').count() == HEADER_LEN));
        assert!(rows[0].contains("resolution_refined_steps"));
        assert!(
            rows[1].contains(
                &sweep.results[0]
                    .as_ref()
                    .unwrap()
                    .metrics
                    .resolution_check
                    .as_ref()
                    .unwrap()
                    .refined_steps
                    .to_string()
            )
        );
        assert!(rows[2].ends_with("intentional failure\""));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(folder.join("results.json")).unwrap()).unwrap();
        assert!(json["results"][0]["Ok"]["metrics"]["resolution_check"].is_object());
        for name in ["results.json", "sweep.csv", "waveform.csv"] {
            fs::remove_file(folder.join(name)).unwrap();
        }
        fs::remove_dir(folder).unwrap();
    }
}
