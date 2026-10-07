## Context

### The pipeline
SteamVR's vrcompositor renders into 3 swap images per eye that the driver allocates through `VRIPCResourceManager`. The driver forwards their dma-buf fds to the presenter.

The presenter, once per refresh:
1. waits until its previous frame reached the display (present-wait);
2. picks SteamVR's newest frame;
3. makes the GPU wait on that frame's write fence;
4. blits each eye into one half of a 3840x1080 swapchain;
5. reports the frame it uses (`USING`), so the driver never hands SteamVR an image the presenter may still read.

The driver declares each vsync 2 ms before the presenter-reported vblank. It can also hold SteamVR in `PostPresent` until the next running start.

### What is measured
Measured on a Steam Deck (Bazzite, KDE Plasma 6 Wayland, AMD RADV Van Gogh, SteamVR 2.18.2 beta), glasses in full SBS with Follow mode and Stabilizer off. See the proposal for the numbers.
- The remaining bad frames are SteamVR's own output.
- The hold removes them only with Home off.
- The hold causes judder: render poses unevenly spaced, double steps.

### Tools available for the next steps
- **`--dump DIR`** captures what the glasses show, inside the frame's own submission, without changing frame timing.
- **`tools/find_bad_frames.py`** lists isolated bad frames and the pointer timeline in a capture. It needs numpy, which is on the Mac, not the Deck.
- **`--sim-pose --sim-yaw 40 --sim-pitch -30 --sim-pitch-amp 0`** pans the gaze pointer across the dashboard toolbar without a wearer. Open the dashboard first with `vrcmd --showdashboard`.
- **The judder report.** The presenter's 5 s report lists the render-pose step median, near-repeats and double steps (code uncommitted on the Mac and built on the Deck).
- **`vrcmd --stats`** gives SteamVR's own submit, present, reprojected, dropped and timed-out counts. Run it from `<SteamVR>/bin/linux64` with `LD_LIBRARY_PATH` set to that directory.

## Goals / Non-Goals

**Goals**
- Zero bad frames in a 480-frame toolbar sweep capture, without the PostPresent hold, so motion is smooth (no double steps in the judder report) and works with Home on or off.
- Present-wait active in every session, not only after a presenter restart.
- A doctor check for whichever workaround is chosen.

**Non-Goals**
- Patching or reverse-engineering vrcompositor beyond loading a Vulkan layer.
- A PostPresent hold tuned to be "good enough". The cap measurements show glitches come back as the hold shortens.

## Decisions

### 1. Try the compositor sync layer first, built from source

**Why the layer.** [SteamVR-for-Linux #952](https://github.com/ValveSoftware/SteamVR-for-Linux/issues/952) is open and reproduces with a fake direct-mode HMD. It documents vrcompositor resetting descriptor pools every frame without GPU sync, and recycling command buffers that are still running (validation errors `VUID-vkResetDescriptorPool-descriptorPool-00313` and `VUID-vkQueueSubmit-pCommandBuffers-00071`).

**Why it fits our frames (inference, not proven).** Stale overlay descriptors would explain all three symptoms:
- a pointer drawn with the wrong transform (huge, smeared);
- a toolbar drawn with the wrong texture coordinates (stretched);
- a missing scene in one eye.

It would also explain why the hold helps: it gives the GPU a frame of slack, so in-flight work finishes first.

**The layer.** [korejan/steamvr-compositor-sync](https://github.com/korejan/steamvr-compositor-sync) (MIT) answers the compositor's reuse poll truthfully and swaps in fresh descriptor pools. It installs to `~/.local` with an implicit-layer override scoped to vrcompositor only.

**Why build from source.** The repo is new and has no stars, and its code runs inside SteamVR's compositor. So we review the source and build it in the usual fedora:44 podman container on the Deck rather than use the prebuilt release.

**Alternatives considered.**
- Running `VK_LAYER_KHRONOS_validation` in vrcompositor only diagnoses the problem. Use it as a confirmation step if the layer's effect is unclear.
- Waiting for Valve's fix: not under our control.

### 2. `steamvr.enableLinuxVulkanAsync` as the second option

It is a plain setting with no third-party code. Its relevance comes from the research agent's reading of #886/#866 (one eye distorted while the dashboard is up) and has not been checked yet. Try it on its own and together with the layer, and record in `docs/findings.md` which one removes the frames.

### 3. A presenter-side bad-frame filter only as a fallback

**How it works.** It is a causal filter: a new frame that differs sharply from the last shown frame is held back for one refresh, and accepted if the next frame still differs. That suppresses isolated bad frames, which is what every capture so far shows, at the cost of one refresh of delay on real scene changes.

**The measure.** Mean absolute luminance difference of a small downsampled copy of each eye, computed on the GPU. Two candidate signals:
- left/right disagreement (the eyes normally match closely);
- a sudden change versus the previous frame.

**Release protocol.** While a frame is held back, the presenter must not send `USING` for it. The previous frame's image therefore stays protected. The existing release protocol already allows the driver to hold two frames: SteamVR keeps one free image.

**Why it's last.** It costs GPU time and latency and treats a symptom.

**Alternative considered.** A one-frame delay buffer that always looks one frame ahead. Rejected because it adds a refresh of latency to every frame.

### 4. Hold default false once a fix lands

The hold is a workaround with a measured cost (judder). Keep `hold_after_present` and `hold_max_ms` as settings for A/B tests, and default the hold to false when task 2 or 3 confirms a fix. The Deck's `steamvr.vrsettings` currently sets it true explicitly; flip it there too.

### 5. Present-wait retry instead of permanent fallback

Startup timeouts happen while SteamVR's windows appear and the compositor reorganises outputs. The fix:
- after 3 consecutive timeouts, stop waiting;
- retry every 5 s with a short timeout until a wait succeeds, then use present-wait again;
- log each switch.

## Hardware dependence

**Specific to the Deck setup (AMD RADV, KDE Wayland, Bazzite):**
- the bad-frame symptom (#952 shows as GPU faults on NVIDIA, which is inference about how it presents on AMD);
- present-wait behaviour under KWin;
- the measured GPU headroom.

**Must hold on any Linux PC:**
- the layer's correctness (vendor-neutral Vulkan);
- the hold default;
- the judder metric;
- the filter.

**Pressure-vessel boundary.** vrcompositor runs inside Steam's pressure-vessel container. The layer manifest under `~/.local/share/vulkan/implicit_layer.d` and the library under `~/.local/lib` must be visible inside it. The README says this works with no launch options; verify via the "active in vrcompositor" log line.

**Settings the user must keep:**
- glasses: full SBS, Follow mode, Stabilizer off, auto-sleep off;
- SteamVR: the standby overrides, `motionSmoothing` false;
- for this change: the Home and background choice, `enableLinuxVulkanAsync`, and `driver_xreal.hold_after_present`.

## Risks / Trade-offs

- [The layer is new, single-author code running inside the compositor] → Review the source before building. Install with `--dry-run` first and keep `--uninstall` ready. It only takes effect in vrcompositor.
- [The layer fixes only part of the problem] → The sweep capture counts what remains. Combine it with the async setting, or fall back to the filter.
- [A future SteamVR update fixes #952 or breaks the layer] → The doctor check and the capture tooling make this quick to re-verify.
- [The filter suppresses legitimate one-frame changes, such as a blink of UI] → Only a single refresh is ever delayed, and it is off unless `--filter-bad-frames` is given.
- [Present-wait retry causes repeated stalls if KWin never reports presentation] → Retry no more often than every 5 s, with a short timeout.

## Migration Plan

Each step is a settings or build change on the Deck. Rollback:
- the layer: `install.sh --uninstall`, or the CMake uninstall equivalent;
- the async setting: remove it from `steamvr.vrsettings`;
- the hold default: set `driver_xreal.hold_after_present` true.

## Open Questions

- Does `enableLinuxVulkanAsync` engage at all under driver direct mode?
- Is the residual judder with the hold off, which shows some double steps, from SteamVR's pose sampling, our pose message timing (sent every 2 ms when new) or the 2 ms running-start lead? Measure once the glitches are gone.
- Does SteamVR Home stay at 60 with the hold off and the layer active? Check with `vrcmd --stats`.
