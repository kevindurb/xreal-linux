# Tasks

## 1. Splash asset

- [x] 1.1 Add `presenter/splash/make_splash.py` (standard library only, embedded 5x7 bitmap font) that writes the coverage patch `splash.raw` plus its width and height, and a `--png` preview. Verify: running it twice gives byte-identical output, and the preview PNG shows two lens shapes above "Waiting for SteamVR" on a flat background (open it and check).
- [ ] 1.2 Commit the generated `presenter/splash/splash.raw` and its size file. Verify: `git ls-files presenter/splash` lists them and the raw size equals width x height.

## 2. Splash logic

- [x] 2.1 Add `presenter/src/splash.rs` with the fade level (smoothstep over about 1 s, 0 on the first frame), patch scaling into staging bytes, and centring offsets for each half; declare it in `main.rs`. Add unit tests for: first frame is 0, level reaches 1 and stays, scaling at 0 and 1, the two halves get the same offset within their half, and the patch byte count matches its dimensions. Verify: `cargo test` in the podman container passes (as for `warp.rs`).

## 3. Presenter integration

- [x] 3.1 Add the staging buffer and splash record function in `main.rs` (clear, then copy the patch into both halves with the layout barriers `record_pattern` uses), and track when the splash began, clearing it when a frame presents SteamVR's eyes. Verify: with no driver, a `--dump` capture shows identical left and right halves (compare the two halves byte for byte), a dark background, and no pixel at the view border or the centre line brighter than the background.
- [x] 3.2 Make the splash the default for "no usable frame" and keep the fallback-frame count behaviour. Verify: with the driver connected and SteamVR not yet drawing, the log's fallback frame count rises while the splash shows, and stays 0 once SteamVR presents.
- [x] 3.3 Add `--test-pattern`: parse it, show the window without a driver (as `--test-grid` does), draw `record_pattern` ahead of the eye images and the grid, and update the header comment and usage line. Verify: `xreal-presenter --test-pattern` draws the old red/blue pattern with or without SteamVR, and a run without the flag never does (a `--dump` capture has no red-tinted or blue-tinted half).
- [ ] 3.4 Check the fade in a dump series. Verify: the first dumped splash frame is all black, the brightness rises monotonically over about a second, and the last frames equal the full-level splash; after killing and restarting SteamVR the fade plays again.

## 4. Docs

- [x] 4.1 Update `presenter/README.md` (the intro and the paragraph describing the red and blue halves become a splash description plus a `--test-pattern` section next to the test grid, and the options list) and the presenter `description` in `Cargo.toml` if it says test pattern. Verify: `grep -n -i "test pattern" presenter` shows only the `--test-pattern` section and the flag.
- [x] 4.2 Update `docs/handoff.md` (the open item "second-distro test pattern" now means `--test-pattern`) and add a short entry to `docs/findings.md` recording what the splash is and what was measured. Verify: the handoff row reads correctly and the findings entry separates what was measured (dump checks) from what was only the wearer's report.
- [x] 4.3 Check the in-flight `support-other-gpus` change for "test pattern" wording (`proposal.md`, `tasks.md` 2.3, `specs/gpu-selection/spec.md`) and note in `docs/handoff.md` that for a GPU mismatch it now means the splash. Verify: the note is there; those files are not edited by this change.

## 5. Check on the Deck

- [ ] 5.1 Build the presenter in the podman container on the Deck, run it with the glasses in full SBS and without SteamVR, then start SteamVR. Verify (wearer): the splash fades in from black, is the same in both eyes, nothing moves, and SteamVR's picture replaces it cleanly. Record what the wearer said in `docs/findings.md`.
- [ ] 5.2 Wearer comfort check: look at the splash for a few minutes, compare with `--test-pattern`. Verify: the wearer reports no nausea from the splash; if not, adjust the level, fade and (if it helps) disparity constants in `splash.rs` and repeat. This is a user check and cannot be done from a frame dump.
