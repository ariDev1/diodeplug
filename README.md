# Diode-Plug Lab

A desktop circuit simulator. Build and run it locally with Rust.

## Install and launch

1. Install [Rust with rustup](https://rustup.rs/) (Rust 1.95 or newer). On Linux,
   use an X11 or Wayland desktop with OpenGL support. On Debian/Ubuntu, install
   the build dependencies with:

   ```sh
   sudo apt install build-essential pkg-config libx11-dev libxcb1-dev libwayland-dev libxkbcommon-dev libgl1-mesa-dev
   ```

2. Clone and start the app:

   ```sh
   git clone https://github.com/ariDev1/diodeplug.git
   cd diodeplug
   cargo run --release --locked
   ```

The first build downloads dependencies and may take a few minutes. Afterward,
you can start the built app directly with `./target/release/diode-plug-lab`
(Windows: `target\release\diode-plug-lab.exe`).

## Getting started

The app opens in **Live**. Choose a preset in the left panel, then use **Play**,
**Pause**, and **Restart from zero** to explore it. Use **Run sweep** to save
results. For explanations and limitations, open the offline **Docs** tab or
read [the circuit guide](diode-plug_explained.md).

For a command-line run without a window:

```sh
cargo run --release --locked -- --batch example-config.json --output results/my-run
```

Results are simulations, not hardware validation.
