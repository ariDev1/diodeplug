# Diode-Plug Laboratory Simulator

Desktop and command-line Rust simulator for the diode-plug circuit in
[`diode-plug_explained.md`](diode-plug_explained.md). Sweep plug capacitance,
inspect waveforms and energy flow, compare circuit presets, and export results
as CSV and JSON. The simulator runs locally; circuit calculations use the CPU.

## Quick start

Install [Rust](https://www.rust-lang.org/tools/install) 1.95 or newer. On Linux,
the desktop app needs an X11 or Wayland session and OpenGL support.

```sh
cargo run --release                              # Open the desktop app
cargo run --release -- --list-presets             # Show built-in examples
cargo run --release -- --batch example-config.json --output results/my-run
cargo test --locked                               # Run the test suite
```

Batch output includes `results.json`, `sweep.csv`, and `waveform.csv`.
The `results/` directory is ignored by Git, as are local build artifacts.
Results are numerical estimates, **not** a validated substitute for bench
measurements; review solver checks and power-balance warnings before use.

## What you can configure

Enter the source voltage and frequency, load R/L/C, source and diode parameters,
and a capacitance sweep. The plug capacitors are equal by default;
the advanced C2/C1 ratio allows unequal values.
See [MODEL_COVERAGE.md](MODEL_COVERAGE.md) for a section-by-section comparison
with the explanatory note and important electrical sign conventions.

## Build and run

Install a recent Rust toolchain (Rust 1.95+). On Linux, the desktop requires an
X11 or Wayland session and its usual OpenGL libraries.

```sh
cargo run --release
```

The application version is taken from `Cargo.toml` (currently **0.4.0**) and
appears in the desktop title and bottom status bar. Use
`cargo run --release -- --version` in scripts. The single-line bottom bar shows sweep progress and
elapsed time while computing; after completion it reports **compute-only**
time, workers used, total points, points per second, solver failures, and
points marked `CHECK` by the numerical quality test. These are observed
wall-clock figures for the current run, not a hardware utilization estimate.
Hover over the status message after a run to see the full output directory;
the sidebar scrolls independently while **Run sweep** remains visible in the
existing header. The Docs tab hides the sidebar and run action for reading.
The header groups the run state, analyzed source settings, sweep range, and
selected point's load response into a compact dashboard. Hover a truncated
header value to read it in full on narrow windows.
The header credits Aether Research Institute without opening a website.

Enter measurements in the left-hand panel, set **CPU workers** to `0` for the
available logical CPU count (or enter a specific number), and press **Run sweep**.
A sweep uses independent CPU workers, up to the number of capacitance points;
`0` automatically uses the logical CPUs available to the process. Very short
runs may finish before CPU-usage monitors update, and hyperthreading does not
necessarily double throughput on compute-heavy circuits.
A progress bar tracks independent capacitance calculations. The table
reports load voltage, load power, input impedance, reflection and numerical
power-balance error; click **any cell** in a successful row to inspect its interactive
steady-state waveform. The point selector beside the tabs and its previous/next
buttons switch between successful points from every analysis view, skipping
failed solves. Plot hover shows the nearest *computed* capacitance point or
*recorded* waveform sample and the relevant values; it does not interpolate
missing data. Click any sweep plot to select the nearest successful computed
capacitance point (failed points are skipped); the selector and analysis views
update together. The selected table row scrolls into view when selection changes,
but manual table scrolling is left alone. A gold marker identifies the selected
computed point on each sweep plot; a compact summary above the plots repeats its
key results. Each new sweep resets
prior plot zoom and table scrolling; switching away and back within the same
run retains the plot view for that selected point. Sweep plot ticks display
physical µF values even though their spacing is logarithmic. Individual failed
points are listed rather than
discarding the whole sweep. The GUI starts with the 10 kHz parameters in the
reported example; substitute your measured parameters before interpreting a
different setup.

On the **Energy** tab, moving over an energy or power plot shows a measurement
cursor at the nearest recorded waveform sample. Color-matched markers and a
compact readout show each trace with an appropriate J/W SI prefix, including
the sign of instantaneous power. These are sampled values, not interpolated
ones.

Worker count is the maximum parallelism for independent sweep points, **not**
the number of cores busy throughout a run. After the points finish, writing
`results.json` is single-threaded. The status bar distinguishes computation
from saving, and the reported compute time excludes export time. Result JSON
uses buffered output; this changes neither recorded samples nor metrics.

To start a new scenario, use **Reset to defaults** at the top of Configuration
and confirm the change. It restores editable circuit, sweep, tank, measurement,
and save inputs; it does **not** discard completed simulation or imported
measurement results. **Save & identify this run** near the bottom of the input
form is optional bookkeeping: Operator and Run label are written to
`results.json`, and the run label is appended to the new timestamped output
folder. **Save runs under** is the parent folder, `results` by default.

The **Docs** tab switches to a full-width reading workspace with a centered,
larger-text article and a section navigator. It bundles rendered versions of
`diode-plug_explained.md` and `MODEL_COVERAGE.md`. Equations use offline SVG
and a bundled math font (no network or external TeX installation). Figure 1
uses the supplied `diode-plug_simple.svg`; the offline-compatible copy
`diode-plug_offline.svg` can be regenerated with
`python3 scripts/prepare-doc-figure.py` without altering the original. The
load connects across the P–N terminals in the supplied drawing. Other source
figures not present in the folder are still marked as unavailable. Model
coverage explains which claims in the original exploratory document are not
simulated.

After each sweep the simulator independently recomputes selected power, RMS,
loss, and cycle-energy metrics from the saved voltage/current samples. Failed
cross-checks are recorded per point in `results.json` and `sweep.csv` and mark
the point `CHECK`. Every solved point is also rerun from a separately settled
periodic state at **twice** its time resolution. Questionable points keep
doubling until two adjacent grids agree or reach the **80,000-step-per-period**
safety limit. Source, port and load power plus input/load current and load
voltage RMS must agree to within 1%; the accepted grid must pass its own
waveform checks and 1% energy-balance test. A stable finer grid replaces the
coarse waveform and metrics; if no adjacent passing pair exists, the original
point remains `CHECK`. Small-signal denominators have a 0.1%-of-largest-signal
floor to avoid unstable percentage comparisons close to zero. Grid-by-grid
metric differences, balance, trend, and accepted resolution are recorded in
`results.json` and summarized in `sweep.csv`. A finer solve that fails is
**CHECK**, not a pass.
The orange **REVIEW REQUIRED** banner means at least one point failed its
consistency, convergence, or 1% energy-balance check (or did not solve).
`COMPUTED` only means the run finished: passing internal checks is **not**
instrument calibration, physical validation, or certification for lab use.

**Lab presets:** the selector offers 22 examples, including 240 V/20 W,
230 V/60 W, 120 V/60 W, 12 V/5 W and 24 V/10 W incandescent bulbs;
a 24 V/24 W heater; a 24 V solenoid; an 8 Ω speaker coil; an RC fixture;
a 5 µH test coil; 24 V and 12 V battery charging proxies; and 30 V/3 A and
12 V/1 A LED arrays, plus a powered AC-side return example for studying
negative measured conductance, a high-impedance 240 V/50 Hz doc example,
and an unequal-capacitance example. Selecting a preset fills the source, load, diode and
sweep fields; you can edit any value afterwards. Bulb resistance is computed
from its *rated* hot operating point, not a filament-temperature model.
The battery uses fixed EMF + internal R with ideal reverse-current blocking;
the LED uses a knee voltage + dynamic R with blocking. A 30 V / 3 A LED
template, for example, uses a 27 V knee + 1 Ω (3 A **at 30 V**), not a
constant-current driver. Battery/LED branch inductance must be zero. The
template AC supply is a specified test fixture, not the battery's DC voltage.

Desktop electrical inputs accept SI prefixes and unit suffixes, for example
`5uH` / `5µH`, `10pF`, `1mOhm` / `1mΩ`, `2.4kΩ`, `240V` and `50Hz`.
Bare numbers use the displayed SI unit; **bare C1/C2 sweep entries are µF**,
and entering `10pF` there converts to `0.00001 µF`. JSON batch configuration
always uses numeric SI fields (plug sweep bounds in µF).

For reproducible headless runs, edit [`example-config.json`](example-config.json)
and use a **new** output directory:

```sh
cargo run --release -- --batch example-config.json --output results/experiment-001
```

To use a built-in preset in a scripted run:

```sh
cargo run --release -- --list-presets
cargo run --release -- --preset 1 --output results/bulb-240v-20w
```

Preset numbers start at 1. For a modified preset, select it in the GUI and
edit the values, or copy its values to a JSON batch file; CSV/JSON results
include the actual numerical inputs and load model used.

The GUI automatically saves each run under the entered results directory in
its own timestamped folder. Both modes write `results.json` (full inputs, worker
count, solver version, every point and waveform), `sweep.csv` (including
per-point errors), and `waveform.csv` (best qualified match, or the middle
point). Existing nonempty result directories are never overwritten. Enter an
operator and sample ID to include them in the record. Inputs use **V RMS**,
Hz, Ω, H, F, and **µF for each plug capacitor**. `results.json` contains every
sample's capacitor currents and energies, diode currents and input/load
instantaneous power. `waveform.csv` includes those traces for one point.

## Performance and GPU

The numerical engine is Rust. The sweep uses Rayon CPU workers; points are
independent and output order remains deterministic. `0` uses the available
logical CPUs, but never launches more workers than sweep points. For an
Intel i7-7700 (4 cores / 8 threads), release-build **compute-only** medians
across three runs of the 12 V / 5 W vehicle bulb preset at 128 points were
0.452 s (1 worker), 0.220 s (2), 0.109 s (4), and 0.091 s (8). On the heavier
dynamic-source preset at 16 points they were 12.34 s (1), 6.33 s (2),
3.87 s (4), and 3.85 s (8): extra logical threads do not always help.
Hardware and scheduling affect timings, and exports take additional time.
The OpenGL desktop renderer may use a GPU to draw the interface, but the
**circuit calculations run on CPU**. This machine has a Radeon 540-series
graphics card but no available OpenCL compute platform. A GPU compute backend
would require a separate, numerically validated implementation of the
nonlinear periodic solver, followed by benchmarking; enabling rendering on
the GPU does not accelerate the simulation.

## Circuit model

`A → C1 → P`, `A → C2 → N`, D1 conducts `B → P`, D2 conducts `N → B`,
and the load is connected between P and N. Load R and L form a **series**
branch in **parallel** with load C in linear mode. LED and charging-battery
modes replace that R–L branch with a blocking knee/EMF + R branch. The source is an ideal sine wave with
series Rs; the diodes have a forward knee Vf and dynamic Rd.

**Optional extended (dynamic) modes.** When diode junction capacitance Cj is
set the clamp node A becomes a dynamic state: a source-side series inductance
Ls (with Vf/Rd/Cj together reproducing the doc's Ls transient discussion) or a
free-ringing parallel tank *replaces* the stiff source algebra for that run.
Also supported: unequal plug caps (C2 = ratio·C1), per-branch capacitor ESR and
parallel leakage, diode reverse recovery `Qrr` (declared charge dissipated at
turn-off) and a pulsed load gate (repetition/duty square wave, phase-locked,
requires L = 0). Cj = 0 with Ls = 0 keeps the original validated stiff model.
Junction capacitance below ~10 nF with 50–100 V excitation is
quadrature-limited; the doc-reproduction preset uses 1 nF and still reports
its balance error per point. Envelope decay is measured from the waveform
peaks, so treat fitted conductances with the sampled count in mind.

The reverse-recovery *current waveform* is not modelled; Qrr contributes only
a declared turn-off loss. Thermal behavior and other load wiring beyond these
optional additions are not modelled. This topology must match the actual
bench circuit for meaningful comparisons. Quadrature note: a pulsed load in
a dynamic (Cj / Ls) mode doubles the period samples (8192) and each step that
straddles a gate transition is subdivided; with Cj = 1 nF this keeps the
power-balance error under ~1% on the validated points.

The solver uses an A-stable implicit midpoint step and a numerical periodic
shooting method for slow-settling loads; natural settling is a fallback. Each
accepted point is checked for periodic-state drift and power balance. Balance
errors above **1%** are shown as `CHECK` and excluded from the matching
recommendation; a non-convergent point is reported as a failure. Solver
resolution is automatically selected from the source frequency and plug
capacitance/source-diode resistance time scale (512–20,000 steps per period,
with additional local subdivisions at difficult switches); the natural
settling fallback limit is 6000 cycles. The chosen period sample count is
recorded for each point. To validate a measured operating point, compare
exported waveforms and port/load powers against measurements.

The **Energy** tab shows stored energies, source/port/load power,
diode conduction and losses, fundamental P1/Q1 and input-current THD.
The **Branches** tab shows node A/P/N voltages, C1/C2 and load-capacitor
currents, clamp-diode pulses, and the source/tank coil current and energy.
The **Tank** tab contains the parallel-tank fundamental loading estimate
(with the doc's series-equivalent resistance Rₛ = ω²L²G/(1+(ωLB)²) and loaded
Q), an explicit **negative-damping risk** flag when the total conductance is
negative, a **weak-coupling screening proxy** ((C1+C2)/C_tank, ratio < 0.1;
not a direct measurement of injected charge), a plug-energy-swing/tank-energy
comparison, a transient **tank ringdown / open-circuit
probe** (free exponential ring with the clamp diodes optional; reports damped
frequency, decay constant and effective conductance G = −2Cα) and the
isolated single-pulse discharge reference. The ringdown is a real transient
simulation of node A; the fundamental estimates remain approximations.

### Signed AC-side power and negative conductance

In the **Powered AC return · negative G example** (preset 15), an explicitly
powered, sinusoidal current-return equivalent is connected at the A–B port.
It represents a separately powered downstream switched-feedback path: enter
the return current RMS and phase relative to the AC source in the sidebar.
The model does **not** deduce that switching waveform from a DC bus or from
the plug capacitors. It computes the port total and passive-plug-only
fundamental G separately and shows negative input-power intervals, returned
joules per cycle, P1 and signed parallel resistance `1/G`. The return path's
electrical power is included in conservation. A negative combined G is thus
an **active-system** measurement, not a passive diode-plug energy gain.
For a reproducible batch example, use
`cargo run --release -- --batch negative-return-example.json --output results/negative-g-001`.
The provided 10 µF point has approximately **Gtotal = −0.00306 S**,
**Gplug = +0.00108 S**, **P₁ = −1.785 W**, and a **2.41 W externally
supplied return path**. Its negative parallel resistance is the *combined*
port's `1/Gtotal`, not a standalone source of energy.
With the return path disabled, temporary negative instantaneous power may
still occur, but a passive periodic steady state cannot deliver sustained
negative real power. See [MODEL_COVERAGE.md](MODEL_COVERAGE.md) for details.

The **Measure** tab can extract G/B and P1/Q1 directly from scope CSVs over an
integer number of cycles. It accepts the simulator's `waveform.csv` columns
`time_s,port_V,input_A` (new exports use `input_total_A`) or
`time_s,voltage_V,current_A`; give it the measurement frequency and choose
**Invert measured current direction** if your probe points out of the port.
It also reports total mean `v·i`, negative-power intervals and returned energy
so a negative **fundamental** P1 is not confused with negative **total** power
when harmonics are present. At least 20 intervals per cycle are required.
The same calculation is available in a scriptable command that prints JSON:

```sh
cargo run --release -- --measure results/experiment-001/waveform.csv --frequency 10kHz
# Add --invert-current if your current probe points out of A–B.
```

Input current is defined flowing **into** the plug. Fundamental input
admittance `Y = G + jB` uses the passive sign convention: positive B is
capacitive. The reflection magnitude is the **fundamental-only** quantity
`|(Zin − Z0)/(Zin + Z0)|`, where the optional positive real `Z0` is the
reference impedance at the A–B plug port. The displayed recommendation is
the lowest-reflection **sampled** capacitor pair that passes the numerical
quality check; a value of V, f, R, L and C alone does not uniquely specify
a best matching capacitor pair. Harmonic reflections are not included.

Run `cargo test` to check the solver, including the 0.449843 µF case that
previously failed to settle, and the ordering of parallel sweep results.
