✈️ FableFlight

An open-world flight simulator written entirely in Rust.

Explore an open world, fly through realistic weather, and share the skies with real-world aircraft. FableFlight combines real aviation data, dynamic weather, and a customizable flight experience in a Rust-powered desktop simulator.

Created with assistance from Claude Fable 5.2 and Claude Opus 5.5.

<p align="center">
  <a href="https://github.com/Rolandjg/fableflight">
    <img src="https://img.shields.io/badge/Language-Rust-orange?logo=rust" alt="Rust">
  </a>
  <a href="https://github.com/Rolandjg/fableflight">
    <img src="https://img.shields.io/badge/Graphics-wgpu-blue" alt="wgpu">
  </a>
  <a href="https://github.com/Rolandjg/fableflight/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/License-Check%20Repository-lightgrey" alt="License">
  </a>
</p>🎮 Demo

See FableFlight in action:

"▶ Watch the FableFlight Demo" (https://github.com/user-attachments/assets/4f8833da-13c0-4d20-a50b-c0acf2d88df1)

✨ Features

🌍 Open-World Flight

- Explore an open-world environment.
- Fly through dynamically changing skies.
- Experience an evolving flight simulation.

🛩️ Live Air Traffic

See real aircraft flying around you using live ADS-B data.

Aircraft information includes:

- Callsign
- Aircraft type
- Altitude
- Ground speed
- Distance from your aircraft

The flight panel, accessible with "Tab", displays nearby aircraft.

Powered by "adsb.lol" (https://adsb.lol). Data is provided under the Open Database License (ODbL).

Live air traffic can be disabled in Settings.

🌦️ Real-World Weather

FableFlight incorporates real-world weather information into the simulation.

Weather data comes from two sources:

- METAR: Uses the nearest available airport weather report within 60 km through "Aviation Weather Center" (https://aviationweather.gov).
- Open-Meteo: Provides model-based weather when no suitable airport METAR is available.

Weather conditions can include:

- Wind speed and direction
- Wind gusts
- Visibility
- Cloud layers
- Thunderstorms

Weather is refreshed every 10 minutes or after traveling 30 km. Transitions between weather reports are smoothed for a more natural experience.

You can disable automatic weather and configure conditions manually through the weather settings.

☀️ Real-Time Environment

Synchronize the simulation with real-world time.

When enabled:

- The sun follows the current time.
- The date matches the real-world date.
- Solar time is calculated for your flight location.

Solar time may differ slightly from the local legal clock.

Use the "T" and "Y" keys to adjust the time manually. Doing so disables real-time synchronization.

🌐 Languages

FableFlight currently supports:

- English
- French

The interface automatically follows your operating system's language when possible.

You can change the language through Settings on the start screen or pause menu. Your preference is saved between sessions.

You can also specify a language for a single run using "--lang".

⚙️ Customization

FableFlight includes configurable settings for:

- Language
- Sound
- Camera
- HUD
- Anti-aliasing
- Weather
- Live air traffic

Your settings are saved automatically.

---

📥 Download and Installation

There are two ways to obtain FableFlight: download the source code directly or clone the repository using Git.

Important: The project currently provides source code rather than a published precompiled installer. You'll need to compile it yourself using Rust.

Option 1: Download the Source Code (Easiest)

1. Visit the "FableFlight GitHub repository" (https://github.com/Rolandjg/fableflight).
2. Click the green Code button.
3. Select Download ZIP.
4. Wait for the download to finish.
5. Extract the ZIP archive.
6. Open the extracted "fableflight" folder.

You should now have the project files, including "Cargo.toml", "Cargo.lock", and the "src" directory.

Option 2: Clone Using Git

If you have Git installed, open a terminal and run:

git clone https://github.com/Rolandjg/fableflight.git

Navigate into the project directory:

cd fableflight

---

🦀 Installing Rust

FableFlight is written in Rust and uses Cargo to compile and manage dependencies.

You need:

- Rust compiler ("rustc")
- Cargo package manager
- Rust 2024 edition support (Rust 1.85 or newer)

Windows

1. Visit https://rustup.rs/
2. Download and run the Windows Rust installer.
3. Follow the installation instructions.
4. Restart your terminal.

Alternatively, install Rust through the official installer using PowerShell or Windows Terminal.

Linux

Install Rust using the official Rust installer:

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Follow the installation instructions.

Restart your terminal or reload your shell environment.

macOS

Install Rust using the official installer:

https://rustup.rs/

You may also need Apple's Xcode Command Line Tools for native compilation.

Verify Installation

Open a terminal and run:

rustc --version
cargo --version

Both commands should display their installed versions.

Make sure your Rust version is at least 1.85.

---

🚀 Building and Running

Once Rust is installed and you've opened the FableFlight project directory, you're ready to compile the game.

Step 1: Compile the Game

Run:

cargo build --release

Cargo will:

1. Read the project's dependency configuration.
2. Download required Rust dependencies.
3. Compile the source code.
4. Generate an optimized release executable.

The first compilation may take some time because dependencies need to be downloaded and compiled.

Step 2: Run FableFlight

After compilation finishes:

cargo run --release

FableFlight should now launch.

Step 3: Run Without Recompiling

Once the release build exists, you can launch the compiled executable directly.

Windows:

.\target\release\fablegame.exe

Linux:

./target/release/fablegame

macOS:

./target/release/fablegame

The executable is named "fablegame" because that is the package name in "Cargo.toml".

---

🌍 Language Configuration

The game automatically detects your system language.

Currently supported languages:

- English
- French

You can manually select your preferred language in Settings.

You can also launch the game in French for a single session:

cargo run --release -- --lang fr

This does not permanently change your saved language preference.

---

🎛️ Controls and Interface

Key| Function
"Tab"| Open the flight panel and view nearby aircraft
"F3"| Access weather controls
"T" / "Y"| Adjust simulation time manually

Additional settings are available through the start screen and pause menu.

---

💾 Configuration and Saved Settings

FableFlight automatically saves user preferences in "settings.json".

The configuration directory is:

Linux:

~/.config/fablegame/

Settings include:

- Language
- Sound
- Camera
- HUD
- Anti-aliasing

Your settings persist between sessions.

---

🌐 Internet Connectivity

Some features rely on external services.

Feature| Data provider
Live aircraft traffic| "adsb.lol" (https://adsb.lol)
Airport weather| "Aviation Weather Center" (https://aviationweather.gov)
Model-based weather| "Open-Meteo" (https://open-meteo.com)

An internet connection is needed for live aircraft and automatic real-world weather updates.

If external services are unavailable, their corresponding live data may not be available. You can disable automatic weather and configure it manually.

---

🔧 Troubleshooting

Rust or Cargo is not recognized

Make sure Rust is installed correctly and restart your terminal.

Check:

rustc --version
cargo --version

Compilation fails

Try updating your Rust toolchain:

rustup update

Then rebuild:

cargo build --release

Graphics or rendering problems

Make sure your graphics drivers are up to date and that your system supports a graphics backend compatible with "wgpu".

Missing Linux development libraries

Some Linux configurations may require additional system development packages for graphics, audio, or controller support.

Check the compilation error for the specific missing library before installing additional dependencies.

Live weather or aircraft are unavailable

Check your internet connection and the availability of the external data providers.

---

🤝 Contributing

Contributions are welcome!

FableFlight is an evolving open-source project, and there is plenty of room for improvement.

Areas where contributions would be especially appreciated:

- 🛩️ More aircraft models
- ✈️ Improved flight physics and realism
- 🌍 World generation and environmental improvements
- 🌦️ Better weather simulation
- 🎨 Graphics and visual improvements
- ⚡ Performance optimizations
- 🖥️ User interface improvements
- 🎮 Additional controls and controller support
- 🐛 Bug fixes and general improvements

How to Contribute

1. Fork the repository.
2. Clone your fork.
3. Create a new branch for your changes.
4. Make your improvements.
5. Test your changes.
6. Submit a Pull Request.

All contributions, big or small, are appreciated!

---

📜 License and Third-Party Data

Check the repository for its applicable software license before redistributing or modifying FableFlight.

Third-party data providers have their own terms and attribution requirements:

- "adsb.lol" (https://adsb.lol) — Open Database License (ODbL).
- "Aviation Weather Center" (https://aviationweather.gov).
- "Open-Meteo" (https://open-meteo.com).

---

🔗 Links

- Rust Installation: https://rustup.rs/
- Rust Documentation: https://doc.rust-lang.org/
- ADS-B Data: https://adsb.lol
- Aviation Weather: https://aviationweather.gov
- Open-Meteo: https://open-meteo.com
