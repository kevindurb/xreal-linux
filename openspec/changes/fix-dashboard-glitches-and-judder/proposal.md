## Why

The headset now presents SteamVR's newest frame every refresh, but two problems make it unpleasant to use:

- **Bad frames.** SteamVR still composites wrong frames, about 15 in every 8 s of pointer sweeping, when the head-gaze pointer moves on or off dashboard UI.
- **Judder.** The only setting that removes those frames, the driver's `PostPresent` hold, makes motion judder so badly that it feels like a low frame rate.

We need clean frames and smooth motion at the same time, without giving up SteamVR Home.

## What we know (measured on the Steam Deck, SteamVR 2.18.2)

- **The bad frames are SteamVR's own output.** The presenter's `--dump` (a capture of the displayed image, which no longer changes frame timing) shows the same bad frames the wearer sees:
  - one eye loses the scene (black or blue), its dashboard panel is dimmed, and the pointer is sometimes smeared huge;
  - in the same frame the other eye only lacks the pointer.
- **Our side looks fine.** The render pose SteamVR reports for those frames is normal (0.19-0.30 degrees from the current pose, the same as good frames). Frame numbers are sequential. No image was reclaimed during the capture.
- **Home is not the cause.** The frames happen with Home on and with Home off (`steamvr.enableHomeApp` false, `steamvr.background` set to `aurorasky.png`).
- **The hold hides the frames but makes motion judder.**
  - With Home off and `driver_xreal.hold_after_present` true, a 480-frame sweep had 0 bad frames.
  - With the hold on, SteamVR's render poses are unevenly spaced: per 5 s report, 23-95 double steps with the hold versus 0-60 without. The wearer reports this as a low frame rate.
  - SteamVR's own stats still show 60 submits and presents per second, with nothing reprojected or dropped.
- **Shortening the hold doesn't work.** With Home on, capping it with `driver_xreal.hold_max_ms` brings bad frames back: 2 per sweep with no cap, 3 at 8 ms, 7 at 4 ms.
- **Resolution doesn't matter.** Halving it barely changed Home's GPU time (10.5 to about 9 ms per frame).
- **Present-wait fails at startup.** When the presenter starts alongside SteamVR, present-wait times out three times and the presenter permanently falls back to estimating vblanks from acquire. It works when the presenter is restarted after SteamVR is up.

## What we assume, and how to settle it

| Assumption | Experiment that settles it |
|---|---|
| The bad frames are [SteamVR-for-Linux #952](https://github.com/ValveSoftware/SteamVR-for-Linux/issues/952): vrcompositor resets descriptor pools and reuses command buffers the GPU is still using. The hold helps only because it gives the GPU slack. | Run vrcompositor with [korejan/steamvr-compositor-sync](https://github.com/korejan/steamvr-compositor-sync) (built from source after review), hold off, and run the same sweep capture. Zero bad frames would confirm it. The layer's own summary line also counts how much reuse it prevented. |
| `steamvr.enableLinuxVulkanAsync: true` fixes a similar one-eye dashboard bug (#886/#866, reported by the research agent and not yet checked). | Set it with the hold off, check `vrcompositor-linux.txt` for async being active, and run the sweep capture. |
| The judder comes from the hold, not from our pose or vsync timing. | With the hold off and no bad frames, which needs one of the fixes above, the presenter's render-pose step report should show no double steps during a smooth simulated pan. |
| Present-wait times out at startup because the window isn't shown or composited yet, not because KWin lacks presentation feedback. | Log when the timeouts happen relative to the first frame, and retry later instead of disabling for the rest of the run. |

## What Changes

1. **Glitch fix, in order of preference:**
   - (a) document and check the vrcompositor sync layer as part of setup, if it fixes the glitches;
   - (b) otherwise `steamvr.enableLinuxVulkanAsync`;
   - (c) otherwise a presenter-side filter that shows the previous frame in place of a detected bad frame.
2. **Hold default:** `driver_xreal.hold_after_present` defaults to **false** once (a), (b) or (c) removes the glitches. Until then it stays true as the glitch workaround. The hold and `hold_max_ms` stay available for diagnosis.
3. **Present-wait recovery:** after timeouts the presenter retries present-wait periodically instead of switching it off for the rest of the run.
4. **Judder metric:** the presenter's 5 s report includes the render-pose step median, near-repeats and double steps (already implemented, uncommitted), so smoothness can be measured without a person wearing the glasses.
5. **Setup:** `tools/doctor.sh` reports whether the sync layer is installed and active in vrcompositor, and the Home-off aurora background is documented as an option.

## Non-goals

- Fixing vrcompositor itself or reporting upstream (beyond optionally adding our data to #952).
- Supporting a PostPresent hold that removes the glitches without judder. The measurements show caps trade one for the other.
- Changing SteamVR's render resolution to work around the glitches.
- Positional (6DoF) tracking, or reprojection changes beyond what is needed to test.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `steamvr-driver`: the PostPresent hold default changes and gains the `hold_max_ms` cap.
- `presenter-display`:
  - present-wait recovers from startup timeouts;
  - the report gains render-pose smoothness;
  - adds the bad-frame filter, if it is needed.
- `glasses-setup`: the compositor sync layer (or the Vulkan-async setting) becomes a checked precondition, and the Home-off background is documented.

## Impact

- `driver/src/xreal_driver.cpp`: Settings, Pacing and PostPresent. `hold_max_ms` already exists, uncommitted.
- `presenter/src/main.rs`: the present-wait retry, the render-pose step report (uncommitted) and the optional filter.
- `tools/doctor.sh`, `driver/README.md`, `docs/findings.md`, `docs/open-questions.md`.
- **New external dependency (user-installed, not vendored):** the `VK_LAYER_STEAMVR_compositor_sync` Vulkan layer, MIT licensed. It is new, with zero stars, so review its source and build it ourselves.
- **SteamVR settings** on the Deck: `steamvr.enableHomeApp`, `steamvr.background`, `steamvr.enableLinuxVulkanAsync`, `driver_xreal.hold_after_present`, `driver_xreal.hold_max_ms`.
