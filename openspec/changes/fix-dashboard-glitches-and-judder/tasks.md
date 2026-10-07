## 1. Land the uncommitted diagnostics

- [ ] 1.1 Commit the `driver_xreal.hold_max_ms` cap (driver Settings, Pacing, PostPresent, activation log) and the presenter's render-pose step report (median, near-repeats, double steps). Document `hold_max_ms` in `driver/README.md`.
- [ ] 1.2 Verify on the Deck: the driver log line shows "hold after present ... (max ... ms)". A 20 s `--sim-pose --sim-yaw 40` run prints render-pose step stats in `/tmp/presenter.log`.

## 2. Compositor sync layer (preferred fix)

- [ ] 2.1 Review the source of korejan/steamvr-compositor-sync (src/, manifest/, scripts/install.sh) for what it intercepts and writes. Note anything surprising in `docs/findings.md` before installing.
- [ ] 2.2 Build it from source in a fedora:44 podman container on the Deck (cmake, ninja, vulkan-headers, vulkan-loader-devel). Install to `~/.local`, using `install.sh --dry-run` first if you install with the script.
- [ ] 2.3 Restart SteamVR with Home off, the aurora background and `hold_after_present` false. Confirm the layer's "active in vrcompositor" line in `~/.local/share/Steam/logs/vrcompositor-linux.txt` (this checks the pressure-vessel visibility).
- [ ] 2.4 Verify:
  - open the dashboard with `vrcmd --showdashboard`;
  - run the 480-frame toolbar sweep capture (`--sim-pose --sim-yaw 40 --sim-pitch -30 --sim-pitch-amp 0 --dump /tmp/dump --dump-frames 480`);
  - run `tools/find_bad_frames.py` on the Mac;
  - record the bad-frame count and the layer's prevented-reuse summary.
- [ ] 2.5 Repeat 2.4 with Home on.
- [ ] 2.6 User check: the wearer confirms no flicker on the toolbar sweep and smooth motion with the hold off.

## 3. Vulkan async setting (second option, or combined)

- [ ] 3.1 Set `steamvr.enableLinuxVulkanAsync` true, with the layer disabled (or uninstalled) for a clean A/B test. Restart SteamVR and check `vrcompositor-linux.txt` for whether async is active under direct mode.
- [ ] 3.2 Verify with the same sweep capture and bad-frame count as 2.4, then again with both the layer and the async setting on.
- [ ] 3.3 Record in `docs/findings.md` which combination removes the bad frames, with Home on and with Home off.

## 4. Hold default

- [ ] 4.1 If task 2 or 3 gives zero bad frames without the hold, change the driver default of `hold_after_present` to false and remove the explicit `true` from the Deck's `steamvr.vrsettings`. Update `driver/README.md`.
- [ ] 4.2 Verify:
  - the driver log says "hold after present off";
  - the judder report during a smooth simulated pan shows near-zero double steps apart from turnarounds;
  - `vrcmd --stats` shows Home at about 60 submits/s with no reprojected or dropped frames.

## 5. Present-wait recovery

- [ ] 5.1 Replace the permanent fallback in `Gfx::draw`: after 3 consecutive timeouts, use the acquire estimate and retry present-wait at most every 5 s with a short timeout. Log each switch.
- [ ] 5.2 Verify with `tools/vr_session.sh start` (presenter and SteamVR started together). Within about 10 s of SteamVR being up, `/tmp/presenter.log` shows present-wait in use again. The driver log shows no "reclaiming" lines after the startup second.

## 6. Bad-frame filter (only if tasks 2 and 3 do not remove the frames)

- [ ] 6.1 Add `--filter-bad-frames`. Compute a small downsampled luminance copy of each eye on the GPU before the display blit, and hold back a new frame that differs sharply from the last shown one (or whose eyes disagree) for one refresh. While holding, do not send `USING` for the held-back frame.
- [ ] 6.2 Verify:
  - the sweep capture with the hold off and the filter on contains no bad frames;
  - the presenter report counts held-back frames;
  - opening the dashboard still appears within one refresh;
  - a user check confirms no flicker.

## 7. Setup and docs

- [ ] 7.1 Add a `tools/doctor.sh` check that warns when neither the layer nor the async setting is active, and reports which one is.
- [ ] 7.2 Verify: run `tools/doctor.sh` with and without the layer installed and check the warning appears only without it.
- [ ] 7.3 Document the Home-off aurora option and the chosen workaround in `driver/README.md`. Move the bad-frame entry in `docs/open-questions.md` to `docs/findings.md` with the measured result.
- [ ] 7.4 Update the "KNOWN OPEN BUG" note in `openspec/config.yaml` to match the outcome.
