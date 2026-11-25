# Virtual reality (OpenXR)

FableFlight's experimental PC VR mode renders both eyes at the runtime's recommended per-eye resolution, with 6DOF head tracking. The live desktop mirror runs without VSync and keeps menus and settings clickable.

[Launch](#launch) · [Windows](#windows) · [Linux](#linux) · [Cockpit controls and performance](#cockpit-controls-and-performance) · [Troubleshooting](#troubleshooting)

## Launch

1. Set up your OpenXR runtime for [Windows](#windows) or [Linux](#linux).
2. Start the runtime and make sure the headset is connected and awake.
3. Run `cargo run --release -- --vr` or launch the built game with `--vr` (for example, `fablegame.exe --vr`). Add `--no-menu` to go straight to the cockpit.

You can also enable **Start in VR mode** in `fableflight-launcher`, or toggle VR under **Settings → Camera → Virtual Reality** and restart. Check the main menu banner or that settings panel for connection and runtime status.

## Windows

Use a Direct3D 12-capable GPU with current drivers and put `openxr_loader.dll` on your PATH or beside `fablegame.exe`. Meta Quest Link, SteamVR, and Windows Mixed Reality provide the loader. Standalone copies are available from the [Khronos OpenXR SDK releases](https://github.com/KhronosGroup/OpenXR-SDK-Source/releases).

| Runtime | Setup |
|---|---|
| **Meta Quest Link / Air Link** | Connect your Quest in the Meta Quest Link PC app over USB or Air Link. Under **Settings → General → OpenXR Runtime**, select **Set Meta Quest Link as active**. |
| **Virtual Desktop** | In the Windows Streamer app, set **OpenXR Runtime** to **VDXR** (recommended) or **SteamVR**, then connect from the Quest app. |
| **SteamVR** | For Index, Vive, Bigscreen Beyond, or Quest via Steam Link or ALVR, start SteamVR and confirm tracking. Under **Settings → OpenXR**, select **Set SteamVR as OpenXR Runtime**. Enable **Advanced Settings** if needed. |

On dual-GPU PCs, add `fablegame.exe` under Windows **Settings → System → Display → Graphics** and select the **High Performance** GPU if it is the one your runtime uses. The game and runtime must use the same D3D12 adapter.

To override the active runtime for one session, set `XR_RUNTIME_JSON` to its manifest, such as `C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json`.

## Linux

### Install the loader and Vulkan drivers

FableFlight uses Vulkan and loads `libopenxr_loader.so.1`. The commands below also install ADB tools for USB Quest setups.

| Distribution | Loader and tools | Vulkan driver |
|---|---|---|
| **Debian, Ubuntu, Pop!_OS, Mint** | `sudo apt install libopenxr-loader1 vulkan-tools android-tools-adb` (run `sudo apt update` first) | AMD/Intel use `mesa-vulkan-drivers`. NVIDIA uses the proprietary driver package. |
| **Arch, EndeavourOS, CachyOS, Manjaro, SteamOS** | `sudo pacman -S openxr vulkan-tools android-tools android-udev` | AMD uses `vulkan-radeon`, Intel uses `vulkan-intel`, and NVIDIA uses `nvidia-utils`. |
| **Fedora, Nobara, Bazzite** | `sudo dnf install openxr-libs vulkan-tools android-tools` | AMD/Intel use `mesa-vulkan-drivers`. NVIDIA uses the RPM Fusion or Nobara drivers. |

### Choose and connect a runtime

- **WiVRn** is the recommended runtime for Quest 2, 3, Pro, and Pico. It connects directly over OpenXR, with Wi-Fi or USB via ADB. Install the server with `flatpak install flathub io.github.wivrn.wivrn`, or install `wivrn-server wivrn-dashboard` from the AUR on Arch (`yay -S` or `paru -S`) or with `sudo dnf install` on Nobara or Fedora with the WiVRn COPR enabled. Install the matching headset client from the Meta Horizon Store or SideQuest, or push it over USB from `wivrn-dashboard`. For USB on Quest, enable Developer Mode, accept **Allow USB debugging** in the headset, and connect from the dashboard.
- **SteamVR** works with native PC VR headsets. Quest needs the ALVR streamer and headset client for Wi-Fi or USB. Prefer native Steam for OpenXR integration (`sudo apt install steam` or Steam's Debian package, `sudo pacman -S steam` with `[multilib]` enabled, or `sudo dnf install steam` with RPM Fusion). Install SteamVR in Steam. For Quest, install ALVR from the [ALVR releases](https://github.com/alvr-org/ALVR/releases) or the Arch AUR (`alvr-bin` or `alvr`), then select **Launch SteamVR** in ALVR Dashboard and connect from the headset. In SteamVR, select **Settings → OpenXR → Set SteamVR as OpenXR Runtime**.
- **Monado / Envision** is a standalone OpenXR option for wired headsets such as Index, Vive, and WMR. On Arch, Envision is available with `yay -S envision-xr-git`.

### Select the active runtime

The loader checks `XR_RUNTIME_JSON` first, then looks for `openxr/1/active_runtime.json` under `~/.config`, `/etc/xdg`, and `/usr/share`, in that order. WiVRn normally sets the active manifest. If native apps cannot find the Flatpak runtime, copy the manifest path from WiVRn's settings and set `XR_RUNTIME_JSON` for the launch.

For a typical native Steam installation, the equivalent SteamVR override is:

```bash
XR_RUNTIME_JSON="$HOME/.local/share/Steam/steamapps/common/SteamVR/steamxr_linux64.json" \
  cargo run --release -- --vr
```

## Menus, globe, and cockpit controls

| Action | Control |
|---|---|
| **Menu laser pointer** | Point either VR controller at the floating 3D menu screen to aim the laser and see the target dot. Pull **Trigger** (or **A / X**) to click; move the active controller's **Thumbstick Up/Down** over scrollable panels to scroll; press **B** (right) or **Right Stick Click** to close a panel or step back, and **Left Menu** to toggle the pause menu. |
| **Flight & radio menu** | In flight, press **B** (right) or **Right Stick Click** to open the floating radial flight menu (Radio, Flight, Go to, Autopilot, Weather, Map, Camera) and quick Pause / Recenter bar. Aim with the VR laser and pull **Trigger** to open the Radio panel, tune VOR / ILS / ADF stations, slew OBS, or adjust the altimeter. |
| **Crash & leaving** | After a crash or wreck, a steady, level-horizon VR card appears with laser-clickable buttons to **Respawn in the Air**, **Choose Location (Globe)**, **Go To Place**, open the **Pause Menu**, or **Quit to Desktop** (or press **Left Menu / B** at any time to open the Pause Menu). |
| **Globe zoom (two-hand pinch)** | On the `Fly` globe screen, squeeze **Grip** on both controllers (or hold both **Triggers** over the open globe) and spread your hands apart to zoom in, pinch them together to zoom out, and move both hands together to pan/circle the globe. |
| **Globe pan & pick** | Squeeze **Grip** on one controller and move your hand to grab and circle the globe directly (or drag **Trigger** on the open globe, or use left stick to pan and right stick to zoom). Tap **Trigger** on the globe to pick a spot or airport; double-trigger to take off. |
| **Search & keyboard** | Click any search box with the VR laser and type on your **physical PC keyboard**, or point and click keys on the compact **VR Keyboard** at the bottom of the screen (click **Hide** if you only want the physical keyboard). |
| **Recenter** | Press **Y** (left) or **Left Stick Click**, press **Home**, middle-click in the cockpit, use your gamepad's view reset, or select **Recenter** under **Settings → Camera → Virtual Reality**. This maps your current physical head pose to the pilot seat while keeping the horizon level. |
| **Seat position** | Press **B** (right) or **Right Stick Click** in flight and use the **Seat** bar (**Back**, **Fwd**, **Down**, **Up**, **Reset**), or adjust **Seat Fore / Aft**, **Seat Height**, and **Seat Left / Right** under **Settings → Camera → Virtual Reality**. Your seat offset moves your eyes and VR hands together and is saved between flights. |
| **Switch seats** | Press **C** to switch between pilot and rear or co-pilot views on multi-seat aircraft. |
| **Reduce reprojection** | Lower **Render Scale** or the **Graphics** preset under **Settings → Graphics**. VR disables TAA and temporal upscalers automatically. **FSR 1** and **Bilinear** remain available. |

## Troubleshooting

- **`OpenXR loader unavailable`** — Check the loader listed for your platform above. On Windows, repair the runtime installation or place the DLL beside the game executable.
- **`no OpenXR headset is available` or the game falls back to desktop** — Confirm the headset is awake and connected before launch, then check the active runtime or set `XR_RUNTIME_JSON` explicitly.
- **`OpenXR requires a different graphics adapter`** — Assign the game to the runtime's GPU in Windows graphics settings.
- **USB Quest missing from `adb devices` or listed as `unauthorized`** — Check Developer Mode in the Meta Horizon mobile app, accept the headset's USB debugging prompt, and try a data-capable cable. Retry with `adb kill-server && adb start-server && adb devices`. On Debian-based systems with permission errors, install `android-sdk-platform-tools-common` for udev rules or add your user with `sudo usermod -aG plugdev "$USER"` and log out and back in.
- **Wi-Fi discovery or streaming fails** — On Arch with WiVRn, install `avahi` and run `sudo systemctl enable --now avahi-daemon`. With Fedora's firewalld, allow the `mdns` service and ports `9757/tcp` and `9757/udp` for WiVRn, or ports `9943/tcp`, `9944/tcp`, and `9944/udp` for ALVR. Use `firewall-config` or `firewall-cmd --permanent` and reload the firewall after applying permanent rules. USB via ADB is also an option.
- **Need startup logs** — Run `RUST_LOG=info cargo run --release -- --vr` to see the selected runtime, Vulkan version, swapchain format, per-eye resolution, and initialization errors.
