# Diode-Plug Laboratory Simulator

A local Rust desktop app for exploring the diode-plug circuit. Edit the source,
load, diodes, and plug capacitors while **Live** integrates evolving startup
voltage and current waveforms; run a capacitance sweep for numerical resolution
checks, deeper analysis, and CSV/JSON exports. Presets and command-line batch
mode are included. Calculations run on the CPU.

## Time-domain Live mode versus sweep

The app opens in **Live**. Play integrates forward in short background chunks
from a zero-energy state and scrolls a bounded oscilloscope history. Pause
freezes simulation time; Restart from zero clears the state and trace. Adjust
the circuit inputs at left or move the C1 slider while running: the next chunk
uses the new values **without resetting the simulation clock or stored state**.
Live and its scope controls work with every supplied preset. Integration and
trigger acquisition continue while you inspect other tabs; return to Live to
view them. Changing the circuit clears an old trigger capture but retains the
selected trigger mode and scope settings (the horizontal range adjusts to the
new sampling interval).
The source phase is continuous across frequency edits. Invalid inputs stop new
integration and display an error instead of a waveform for incorrect settings.
The **Trigger** strip selects source voltage, port voltage, load voltage,
input current, or load current; a rising/falling edge and level determine the
crossing. **Holdoff** sets a minimum simulated-time gap before the next
crossing can capture. The window has 25% pre-trigger history, with both
scopes aligned at interpolated trigger time `t = 0`. **Auto** falls back
to a free-running display before a valid crossing or when edges disappear;
**Normal** waits and holds the last timestamped capture; **Single** freezes
the first capture until **Re-arm**. Switching trigger sources sets the level
to the midpoint of the observed signal range; **Mid-level** recalculates it
after the circuit settles. For the 24 V battery preset, load voltage stays
positive, so a 0 V level cannot trigger it; a rising 0 A edge can capture the
onset of charging current. An out-of-range trigger level is called out in Live.
Each plot has colored on/off signal
selectors plus **All**, **None**, and **Solo…**. A hidden trigger source still
triggers and is marked as hidden. **Auto timebase** follows the configured
source frequency, showing about two cycles over 10 divisions. If a pulse gate
repeats more slowly, Auto follows that rate instead. The displayed ms/div is
the effective scale, and **history limit** appears if the requested span exceeds
the retained history. Turn Auto off for the logarithmic **Horizontal** time/div
slider and 2× zoom buttons; units adapt from ns/div to s/div, selecting a
64–8192-sample window. Live records
a bounded scope history (roughly 3,072 samples per slowest configured cycle when
integration needs finer steps); every solver step still runs. A flat signal or
a level it never crosses cannot edge-trigger, regardless of timebase. Auto or manual V/div
and A/div set the eight-division
vertical scale independently; these display choices do not change samples or
the solver. These are display/acquisition controls: integration continues in
all three modes unless you press **Pause**. Trigger settings never change the
circuit model, sweep results, or saved files.
Changes to ideal component values are external operator interventions: the
integrator retains capacitor voltages and inductor currents, but instantaneous
edits can add/remove stored energy. Simulated time is deliberately shown
separately from wall-clock time; this is **not** guaranteed real-time playback.
Live traces are not saved, do **not** undergo the sweep's adjacent-grid
resolution or full-cycle energy checks, and are not measured or hardware-
validated. Select **Run sweep** to save and numerically check capacitance
points. Sweep results and Live state remain separate.

## Install and run

1. Install [Rust via rustup](https://rustup.rs/) (Rust 1.95 or newer). On Linux,
   use a desktop session with X11 or Wayland and OpenGL support.
2. Clone this repository and launch the app:

   ```sh
   git clone https://github.com/ariDev1/diodeplug.git
   cd diodeplug
   cargo run --release
   ```

Cargo downloads and builds the dependencies on the first run. Later, launch the
built application with `./target/release/diode-plug-lab` (on Windows,
`target\\release\\diode-plug-lab.exe`).

For a headless example: `cargo run --release -- --batch example-config.json --output results/my-run`.
Run checks with `cargo test --locked`. Output under `results/` is not tracked by Git.

## Documentation

Use the **Docs** tab in the app or read the [circuit explanation](diode-plug_explained.md)
and [model coverage](MODEL_COVERAGE.md) for details and limitations. Results are
simulation estimates, not hardware validation; review numerical warnings before
using them to guide measurements.
