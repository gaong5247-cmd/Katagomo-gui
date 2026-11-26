# KataGomo Studio / Katagomo-gui

Native Rust + Tauri 2 local Gomoku analysis workbench using the real [KataGomo](https://github.com/hzyhhzy/KataGomo) C++ neural engine. No Piskvork/QLizzie application required. Early implementation; the native build and real model tests must pass before a stable release.

## Features

- 15×15 / 19×19 board, freestyle, standard, Renju, Caro
- Per-rule **and per-board-size** executable, config, model mapping. No silent fallback to an unrelated network.
- Real KataGomo JSON analysis, interim results, visits, black-perspective expected result (win + half draw), candidate draw rate, PV, candidate overlay, history plot.
- Human/AI moves, undo, analysis cancellation, SGF export.
- CLI protocol bridge, see [protocol compatibility](docs/protocols.md).
- Pinned upstream source preparation; patch compile-time board limit 15 → 19, correct Caro rule parsing, expose Renju forbidden intersections.

The GUI is an analysis editor, not yet a complete official tournament referee. It does not independently enforce all terminal/forbidden move rules or Swap2. 19×19 is a board/engine capability, not a claim of trained 19×19 networks or equivalent playing strength.

## Windows build

Prerequisites: Rust stable MSVC, Visual Studio C++ Build Tools, WebView2, Node.js for the Tauri CLI, Python 3, Git, CMake, and backend libraries for the engine (Eigen3 + zlib for CPU; CUDA/OpenCL/TensorRT as applicable).

```powershell
git clone https://github.com/gaong5247-cmd/Katagomo-gui.git
cd Katagomo-gui
python scripts/prepare_engine.py --download-renju
# Optional: compile the patched source (configure library paths as needed)
python scripts/prepare_engine.py --build --backend EIGEN
# Generate platform icons, build UI executable
npx --yes @tauri-apps/cli@2 icon assets/icon.svg
npx --yes @tauri-apps/cli@2 build --no-bundle
cargo build -p katagomo-core --release
```

For a faster start on 15×15, use an official compatible executable from [Gomoku release](https://github.com/hzyhhzy/KataGomo/releases/tag/Gomoku_20250206) and the matching model/config. Some release configurations are for GTP; use config/analysis.cfg or an analysis-compatible cfg. For 19×19 and forbidden overlays, build the patched source. Build output paths depend on the generator (e.g. engines/Gom2024/build-eigen/Release/katago.exe or engines/Gom2024/build-eigen/katago).

Select rule and size, browse to engine/model/config, save profile, then press Analyze. Each rule/size uses its saved profile. Export/import the profile list as JSON to reuse exactly the same settings in the CLI. Caro requires the separate Caro2024 engine. Model weights are not included; register only models known to support the selected game and size. GPU settings live in the chosen config and compiled engine backend.

## Linux

Install Tauri prerequisites (WebKitGTK 4.1, GTK3, appindicator, librsvg, etc.) plus Eigen3/zlib for a CPU engine build. Use the same Python and npx commands. Core tests: cargo test -p katagomo-core.

## GitHub Actions

The Build and test workflow produces the Windows UI and bridge artifacts; Patched KataGomo engine builds the Eigen CPU engine on Windows and Linux. Download both successful workflow artifacts from the Actions tab. The optional real_model_smoke workflow input tests real official Renju network inference on both sizes, but does not establish that the network was trained for 19×19.

## CLI

Copy profiles.example.json to profiles.json and replace all placeholder paths with actual paths. Paths resolve relative to the launch working directory.

```powershell
cargo run -p katagomo-core --bin katagomo-bridge -- profiles.json
```

Raw GTP or JSON pass-through (engine's native protocol):
```
katagomo-bridge profiles.json --gtp renju 15
katagomo-bridge profiles.json --json renju 15
```
Raw modes select the model/executable/config but the caller must set board size and rules in GTP/JSON itself. In GTP use boardsize 15 and kata-set-rules {"basicrule":"RENJU"}. For Caro use the patched Caro2024 engine and {"sixwinrule":"SIXWINRULE_CARO","wallblock":false}.

## Upstream and license

KataGomo is derived from KataGo, with its original license/attribution preserved in each fetched source tree. This repository fetches and patches upstream commits rather than copying model binaries or stripping upstream notices. See scripts/prepare_engine.py for pins and patches. Rapfi and Yixin are interoperability references, not bundled engines. No claim of complete Rapfi protocol compatibility.
