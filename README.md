# Diode-Plug Lab

A desktop circuit simulator. Build and run it locally with Rust.

## Install and launch

You need Rust 1.95 or newer, Git, and a graphical desktop with OpenGL support.
Install the prerequisites for your system:

### Omarchy (Arch Linux)

Open a terminal in your Omarchy desktop and run:

```sh
sudo pacman -Syu --needed git base-devel wayland libxkbcommon libx11 libxcb mesa
rustc --version
```

If `rustc` is missing, install Arch's Rust toolchain manager and select stable:

```sh
sudo pacman -S --needed rustup
rustup default stable
rustc --version
```

If Rust is already installed, use it if `rustc --version` is at least 1.95;
otherwise update your existing toolchain (for rustup: `rustup update stable`).
No `apt` commands are needed on Omarchy.

### Debian / Ubuntu

Install [Rust with rustup](https://rustup.rs/) (Rust 1.95 or newer), then:

```sh
sudo apt update
sudo apt install git build-essential pkg-config libx11-dev libxcb1-dev libwayland-dev libxkbcommon-dev libgl1-mesa-dev
```

### Run (all systems)

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
results.

Inductance and capacitance fields have a unit selector. A bare number uses the
selected unit (for example `470` with `µF`); an explicit suffix such as
`470uF`, `0.1H`, or `10nF` overrides it. Switching units converts the displayed
number without changing the circuit value.

For component-level work, expand **Advanced · datasheet component data** to
enter diode Vf0, Rd, Cj and Qrr, plus capacitor C2/C1, ESR and leakage. The
diode picker includes common parts such as 1N4007, 1N5408, UF4007, MUR460,
MUR4100E, VS-HFA16PB120-N3, 1N5819 and 1N5822. The capacitor presets cover
CBB65 motor-run capacitors (450 VAC) and WIMA MKP 10, MKP 4 and FKP 1 film
capacitors. A capacitor preset fills **Live plug C1**, ESR and leakage; C2 still
follows the C2 ratio. The separate load-parallel C and sweep limits are unchanged.
Capacitor ESR is calculated from the datasheet tanδ at the stated
frequency, so recalculate it for a different operating frequency. Library
values are typical starting points; check the exact datasheet before treating
a result as hardware validation.

For explanations and limitations, open the offline **Docs** tab or read
[the circuit guide](diode-plug_explained.md).

For a command-line run without a window:

```sh
cargo run --release --locked -- --batch example-config.json --output results/my-run
```

Results are simulations, not hardware validation.
