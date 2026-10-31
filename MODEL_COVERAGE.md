# Model coverage relative to `diode-plug_explained.md`

**Calculation checks are not laboratory validation.** Each solved sweep point
is checked against its recorded voltage/current waveform for selected power,
RMS, loss and cycle-energy metrics. A point marked `CHECK` also exceeds the
1% energy-balance threshold, fails an independent calculation check, or differs
by more than 1% between two separately settled adjacent time resolutions.
Questionable points are rerun at progressively finer grids up to 80,000
samples per period; only a passing grid verified against the next finer grid
replaces the original. Otherwise the point stays `CHECK`. A
computed sweep can contain `CHECK` points; neither passing these internal
checks nor displaying plausible powers certifies the model against hardware.

| Markdown subject | Status in Rust tool |
| --- | --- |
| A–C1–P, A–C2–N; B–P and N–B diode clamps | Time-domain model with Vf/Rd. Plug capacitors are equal by default; optional C2/C1 ratio permits unequal values. Node and diode current waveforms exported. |
| Capacitor voltages, currents and stored energies | Exported per settled sample for C1 and C2. Load C and L energies exported separately. Peak and swing of combined plug capacitor energy in metrics. |
| Load voltage, current, real power | RMS and mean load voltage, branch current, instantaneous electrical power, resistor and LED/battery threshold power. A "20 W / 240 V" incandescent preset is a **2880 Ω fixed hot-filament resistor**, rated 20 W only when the modeled lamp terminal voltage is 240 V RMS. A diode-plug can raise the P–N voltage and the calculated resistor power far above that nameplate; thermal behavior, resistance changes and failure are **not** modeled. LED/battery templates also use simplified blocking models. |
| Source input voltage, current and power, conservation | Source and port waveforms, instantaneous power, source-R and diode losses, power-balance diagnostic including change in stored energy. |
| Diode conduction conditions and pulses | D1/D2 instantaneous, peak and conduction fraction; piecewise-linear forward Vf/Rd model. |
| Fundamental equivalent admittance, P1 and Q1 | Complex G/B and impedance from Fourier projections at drive frequency, fundamental P1/Q1, input-current THD, port reference reflection when real Z0 is supplied. |
| Laboratory extraction of G and B from measured samples | **Measure** tab imports CSV with `time_s,port_V,input_A` or `time_s,voltage_V,current_A` over integer periods, supports probe-direction inversion, and reports signed G/B, P1/Q1, total average real power and returned energy. At least 20 samples per period required. |
| Negative instantaneous input power and negative AC-side fundamental G | Signed source and port power waveforms, fraction of time with negative power, returned joules per cycle and signed G/P1/Rparallel. An optional **externally powered AC-side switched-return equivalent** injects a synchronized current at A–B. Reports plug-only and combined G separately, and explicitly accounts for return-path power. |
| No-load peak-charge and stored-energy approximation | Not treated as an exact law. Use a large R only as an **approximate** open circuit; check charge voltage from actual waveform. |
| Isolated single-pulse capacitor discharge | Experiments → Capacitor pulses computes ideal ½CV² and conditional 2 f·½CV² reference, separate from periodic diode-plug prediction. Does not simulate a disconnected discharge switch. |
| Resonant tank loading, f0, Q | Experiments → Resonant tank combines a specified parallel tank L, C and loss conductance with the **operating-point fundamental** plug G/B. Assumes plug admittance constant near drive frequency. This is a reference calculation, not a coupled resonant transient. |
| Finite source inductance Ls | **Implemented as a transient mode.** With `source_inductance > 0` and `diode_capacitance_f > 0` the clamp node A becomes a dynamic state driven by Ls and Rs (plus an optional tank). The doc's negative-damping discussion (coupled tank + Ls) is reproduced via the tank ringdown probe and series-equivalent formula below. Numeric accuracy is quadrature-limited for very small junction capacitances; the preset demo uses Cj = 1 nF. Without Ls, node A is fixed algebraically and Cj is ignored. |
| C1 ≠ C2, capacitor ESR and parallel leakage | C2 = `capacitor_ratio`·C1 per sweep point; per-branch ESR (loss-only accounting) and leakage (a steady-state loss path) with dedicated loss metrics and CSV columns. |
| Pulsed (gated) load | Optional square-wave gate on the load branch (repetition + duty, phase-locked at t = 0); kept energy-balanced with held-capacitor behavior documented. |
| Diode junction capacitance Cj and reverse recovery | Cj is a dynamic clamp-node capacitance used in the source-inductance mode; without Ls the clamp node is stiff and Cj is not applied. Turn-off switching energy `Qrr·|reverse voltage|` is accumulated event-by-event into a `reverse_recovery_loss_w` metric. |
| Common diode data / advanced datasheet entry | The **Advanced · datasheet component data** section accepts Vf0, Rd, Cj, Qrr, C2/C1, ESR and leakage. A built-in library provides typical fits for 1N4148, 1N4007, 1N5408, 1N4937, UF4007, MUR460, MUR4100E, VS-HFA16PB120-N3, 1N5819 and 1N5822. Library values are starting points, not manufacturer guarantees. |
| Capacitor presets | Presets cover CBB65 motor-run capacitors (450 VAC) and WIMA MKP 10, MKP 4 and FKP 1 film capacitors. A preset fills **Live plug C1**, ESR and leakage; C2 remains `ratio·C1`. The distinct load-parallel C and sweep limits stay unchanged. ESR is derived from the datasheet tanδ at the stated test frequency and is not adjusted automatically when the simulation frequency changes. Leakage is an insulation-resistance estimate. These are typical starting points, not guarantees. |
| Simple power test and optional precharge | **Experiments → Power test** uses the current sidebar circuit and Live plug C1 (never a saved sweep point) to report modeled **settled** mains watts, load watts and RMS voltage across the load with one button. It is not a bench measurement or a real-lamp thermal model; a failed numerical check is marked `CHECK`. Under **Advanced · startup, precharge and cycle-by-cycle energy**, the separate probe compares **source input**, **load consumption** and **stored-energy change** over 1–8 cycles without settling. The first-cycle source wattage is not the load wattage or nameplate rating. Zero initial voltages start uncharged; an optional negative-power example changes editable inputs without altering saved sweeps. Advanced may choose Live plug C1 or the selected sweep point's C1; the sidebar's *Load parallel C* is a separate component. The probe accepts valid sidebar diode/load/return/gate/Ls settings without modifying them. It reports powered AC return/DC-bus contributions and independent energy-balance quality; tiny Cj with Ls can be numerically stiff. Qrr is not a dynamic transient model and is excluded from finite-cycle loss/balance; ESR remains loss-only and is listed separately. Precharge is external initial energy, not steady-state generation. |
| Tank ringdown / open-circuit probe | `solver::ringdown` runs one free transient of node A with an attached parallel tank (L, C, G) with or without the clamp diodes, fits the envelope decay and reports damped frequency, decay constant, effective conductance G = −2Cα and a **negative-damping** flag. |
| Single-pulse discharge | Experiments → Capacitor pulses offers the document's isolated C1→R case with N held at 0 V (τ = R·C1), or two equal capacitors discharging as a series pair into R with diodes off (τ = R·C/2; common-mode pair frozen). Both are ideal reference circuits, not periodic plug predictions. |
| Weak-coupling criterion | Experiments → Resonant tank reports the conservative capacitance proxy (C1+C2)/C_tank with ratio < 0.1 as weak, plus plug-capacitor energy swing relative to ideal tank peak energy. Neither directly measures net injected charge or extracted energy per cycle; this is a screening estimate, not a coupled transient result. |
| Reverse recovery, thermal lamp dynamics, motor back-EMF, SOC/LED driver | Lamp/motor/SOC/driver **not implemented.** Recovery handled as declared charge above. Presets are named laboratory *approximations*, not calibrated device models. |

## Boundaries of document reproduction

The 240 V / 50 Hz / 576 Ω high-impedance example is a configurable preset,
not an exact reconstruction of the document's figures: the document does not
specify numerical Rs, Ls, diode or plug-capacitor values for those plots. The
preset chooses explicit illustrative values and uses Cj to enable the dynamic
source-inductance solver. Actual bench measurements are needed for a quantitative
match. An ideal open circuit is not a steady-state sweep point (R must be
positive); use a large R to approximate no load. The document's disconnected
single-pulse discharge is a separate reference calculation, not the load power
of a continuously driven diode plug.

## Convention and interpretation corrections

For RMS fundamental phasors with current **into** the A–B plug port,
`Y = I/V = G + jB`, `S = V·conj(I) = |V|² (G − jB)`.
Therefore `P1 = |V|² G`, `Q1 = −|V|² B`; **positive B is capacitive**.
`1/G` (when nonzero) is a *parallel* equivalent resistance, whereas the
series equivalent is `Re(1/Y) = G/(G²+B²)`. Some exploratory statements in
the markdown use opposite signs or mix parallel and series equivalents;
the tool uses the definitions here.

The passive steady-state source/diode/load topology cannot supply sustained
negative net real input power without another active supply. Negative
*instantaneous* input power can occur when stored energy returns to the source.
Select **Powered AC return · negative G example** (preset 15) for an explicit
externally supplied, source-synchronous current injector at A–B. The AC-side
meter sees `i_total = i_plug − i_return`, so the total `G = Re(I_total,1/V_1)`
may be negative while the passive `G_plug = Re(I_plug,1/V_1)` remains positive.
The return source is an **equivalent** for powered downstream switching, not a
claim that C1/C2 or the rectifier diodes provide energy. The model does not
implement the switch/coupling path between the downstream DC bus and A–B;
its current amplitude and phase must be specified or measured separately.
Its cycle-averaged electrical power is accounted for explicitly, so a
negative measured AC-side G does not violate conservation.

The bidirectional DC bus load model alone need not produce negative AC G:
in this clamp topology an elevated DC EMF can leave the rectifier clamps
off and decouple the AC port. The charging-only battery preset has ideal
reverse blocking. These modes must not be conflated with an AC-side powered
feedback path.
The separate passive resonant-tank estimate likewise does not establish
oscillation/negative damping. Measured negative long-time input power would
require an explicitly modelled active element or a review of measurement
reference directions and non-steady-state windows.

Reflection in the tool is only the **fundamental** `|(Zin − Z0)/(Zin + Z0)|`
for a real specified port reference `Z0`; harmonics in a nonlinear circuit
are not characterized by a single reflection coefficient. A source frequency
and voltage do not specify tank L/C or port reference impedance; enter them
explicitly where the tool asks for them. With an explicitly powered return path,
`|Γ|` can exceed 1; this indicates power supplied at the fundamental by the
active **combined** port, not extra energy created by its passive capacitors.
