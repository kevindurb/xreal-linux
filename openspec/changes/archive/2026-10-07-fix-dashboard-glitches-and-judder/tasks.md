> **Status (2026-10-07): resolved by a different route than planned.** The wearer confirmed "perfect" (Home on) with hold on, `running_start_ms` 8 (now the driver
> default) and `--reproject` (now on by default in `tools/vr_session.sh`). The sync layer (2.x) and `enableLinuxVulkanAsync` (3.x) were measured and do NOT remove
> the bad frames (layer uninstalled from the Deck); 4.x (hold default false) is not adopted because the hold stays on; the presenter filter (6.x) was dropped by
> decision. Findings: `docs/findings.md`. Left: measure bad frames with the hold off and a later running start (not needed for the fix), run `tools/doctor.sh` check
> 7.2, and archive the change after its specs are updated to match (they still describe the layer/filter plan).

## 1. Land the uncommitted diagnostics

- [x] 1.1 Commit the `driver_xreal.hold_max_ms` cap (driver Settings, Pacing, PostPresent, activation log) and the presenter's render-pose step report (median, near-repeats, double steps). Document `hold_max_ms` in `driver/README.md`.
- [x] 1.2 Verify on the Deck: the driver log line shows "hold after present ... (max ... ms)". A 20 s `--sim-pose --sim-yaw 40` run prints render-pose step stats in `/tmp/presenter.log`.

## 2. Compositor sync layer (preferred fix)

- [x] 2.1 Review the source of korejan/steamvr-compositor-sync (src/, manifest/, scripts/install.sh) for what it intercepts and writes. Note anything surprising in `docs/findings.md` before installing.
- [x] 2.2 Build it from source in a fedora:44 podman container on the Deck (cmake, ninja, vulkan-headers, vulkan-loader-devel). Install to `~/.local`, using `install.sh --dry-run` first if you install with the script.
- [x] 2.3 Restart SteamVR with Home off, the aurora background and `hold_after_present` false. Confirm the layer's "active in vrcompositor" line in `~/.local/share/Steam/logs/vrcompositor-linux.txt` (this checks the pressure-vessel visibility).
- [x] 2.4 Verify:
  - open the dashboard with `vrcmd --showdashboard`;
  - run the 480-frame toolbar sweep capture (`--sim-pose --sim-yaw 40 --sim-pitch -30 --sim-pitch-amp 0 --dump /tmp/dump --dump-frames 480`);
  - run `tools/find_bad_frames.py` on the Mac;
  - record the bad-frame count and the layer's prevented-reuse summary.
- [x] 2.5 Repeat 2.4 with Home on.
- [x] 2.6 User check: done with the final settings instead (see 4.2); the layer was dropped.

## 3. Vulkan async setting (second option, or combined)

- [x] 3.1 Set `steamvr.enableLinuxVulkanAsync` true, with the layer disabled (or uninstalled) for a clean A/B test. Restart SteamVR and check `vrcompositor-linux.txt` for whether async is active under direct mode.
- [x] 3.2 Verify with the same sweep capture and bad-frame count as 2.4, then again with both the layer and the async setting on.
- [x] 3.3 Record in `docs/findings.md` which combination removes the bad frames, with Home on and with Home off.

## 4. Pacing defaults (replaces "hold default false": the hold stays on)

- [x] 4.1 Add `driver_xreal.running_start_ms` and sweep it (2-12 ms, Home on, hold on). Result: 8 ms or more gives no bad frames. Make 8 ms the driver default and document it in `driver/README.md`.
- [x] 4.2 Make `--reproject` the default in `tools/vr_session.sh` (`XREAL_REPROJECT=0` turns it off) after `tools/judder_report.py` showed it removes the judder the hold causes. The wearer confirmed no flicker and smooth motion with Home on.
- [x] 4.3 Make 1920x1080 the default render size after the wearer confirmed it was smooth with Home on.

## 5. Present-wait recovery

- [x] 5.1 Replace the permanent fallback in `Gfx::draw`: after 3 consecutive timeouts, use the acquire estimate and retry present-wait at most every 5 s with a short timeout. Log each switch.
- [x] 5.2 Verify with `tools/vr_session.sh start` (presenter and SteamVR started together). Within about 10 s of SteamVR being up, `/tmp/presenter.log` shows present-wait in use again. The driver log shows no "reclaiming" lines after the startup second.

## 6. Bad-frame filter

Dropped by decision: it hides a symptom, and working PCVR setups do not need it. No `--filter-bad-frames` was added.

## 7. Setup and docs

- [x] 7.1 Add a `tools/doctor.sh` check that warns when `hold_after_present` is false or `running_start_ms` is below 8. (Replaces the planned layer/async check, since neither helps.)
- [x] 7.2 Verify: the check's settings logic was run against four synthetic `steamvr.vrsettings` cases; warnings appear only for hold false or running start below 8.
- [x] 7.3 Document the Home-off aurora option and the chosen workaround in `driver/README.md`. Move the bad-frame entry in `docs/open-questions.md` to `docs/findings.md` with the measured result.
- [x] 7.4 Update the "KNOWN OPEN BUG" note in `openspec/config.yaml` to match the outcome.
