# Tasks

## 1. Vendor-neutral tooling

- [ ] 1.1 Find the glasses' connector from the DRM mode list in `tools/vr_session.sh`, `tools/doctor.sh` and `tools/measure_dashboard.sh` instead of `card1`/`DP-1`. Verify: on the Deck all three still find the glasses and a start/check/measure run behaves as before; a script test feeds a fake `/sys/class/drm` tree with two connectors and finds the right one.
- [ ] 1.2 GPU load reader that uses amdgpu sysfs, `nvidia-smi` or `intel_gpu_top` when present and is skipped otherwise; use it in `measure_dashboard.sh`. Verify: on the Deck the reported load matches the previous numbers; with no counter available the script still completes and says the load is unavailable.

## 2. GPU selection

- [ ] 2.1 Experiment: find what identifies SteamVR's GPU (vrcompositor and vrserver log lines, SteamVR settings, IPC objects, Steam's own device choice). Verify: the findings are written to `docs/findings.md` with the exact source for the Deck, or the statement that none exists.
- [ ] 2.2 Implement selection by identity with the fallback chain (SteamVR's identity, first loader device, manual override via a presenter flag) and verify the choice by the first import. Verify: unit tests with a fake device list for each branch; on the Deck the log names the device and how it was chosen.
- [ ] 2.3 Mismatch handling: log both GPUs and show the test pattern instead of garbage when the match fails; surface it in doctor/`xreal-setup` output. Verify: a forced mismatch (the override pointing at a nonexistent device) produces the log lines and the test pattern on the Deck.

## 3. Synchronisation without a sync file

- [ ] 3.1 Log the active wait method and vblank source once per session and in the periodic report; record per-frame fence support in the capture metadata (already present). Verify: the Deck's log names the fence wait and present wait.
- [ ] 3.2 Measure the writer-completion time on the Deck (time from Present to fence signal) from a sweep session. Verify: the p50/p99 are written to `docs/findings.md`.
- [ ] 3.3 Implement the bounded fallback wait and a debug switch that forces it. Verify: with the switch on, the log says the fallback is in use and the frame age at read is never below the configured wait.
- [ ] 3.4 Compare the fallback against the fence path on the Deck with the same sweep capture (hold on, running start 8 ms, reprojection on, Home off and on). Verify: bad-frame counts for both are in `docs/findings.md`; set the default delay from the result and note the extra latency.

## 4. Compatibility report

- [ ] 4.1 Presenter report mode: devices, extension support, import attempt, active strategies, display mode; no images or personal data. Verify: a unit test asserts the output contains none of a list of forbidden fields; the Deck run prints the expected lines.
- [ ] 4.2 `tools/gpu_report.sh`: runs the report mode plus the sweep capture and analysis and prints a paste-ready report; handles SteamVR or the glasses being absent. Verify: runs to completion on the Deck with everything present and with SteamVR stopped (reporting what it could not test).
- [ ] 4.3 `docs/gpu-compatibility.md`: the supported-hardware table with the criteria, the Deck's report as the first row, and how to submit a report. Verify: the Deck row is backed by a report committed in the doc's format.

## 5. Other hardware (needs access; each ends in a recorded result, working or not)

- [ ] 5.1 NVIDIA (proprietary driver): run the report. Verify: the result, including whether import and the sync-file export work, is in the table with the report attached or linked.
- [ ] 5.2 Intel (Arc or integrated, Mesa ANV): run the report. Verify: the result is in the table.
- [ ] 5.3 A hybrid-GPU laptop with the glasses on each output: run the report. Verify: both outcomes are in the table and the mismatch handling is exercised if the GPUs differ.
- [ ] 5.4 Another AMD GPU (a desktop card): run the report. Verify: the result is in the table.

## 6. Integration

- [ ] 6.1 Re-run the Deck's end-to-end session (start, dashboard use, judder and bad-frame captures) after all changes. Verify: bad-frame counts and judder numbers are unchanged from `docs/findings.md`, SteamVR delivers 60 new frames/s, and doctor passes.
