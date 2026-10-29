# Diode-Plug Laboratory Simulator

A local Rust desktop app for exploring the diode-plug circuit. Configure a source,
load, diodes, and plug capacitors; run a capacitance sweep; inspect waveforms,
power, and energy; and export CSV/JSON results. Presets and a command-line batch
mode are included. Calculations run on the CPU.

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
