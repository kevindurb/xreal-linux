# Design

## Context

The presenter creates a Vulkan instance and window, takes the first physical device that can present to it, imports SteamVR's swap images as opaque-fd external memory, and waits for SteamVR's writes by exporting a sync file from each image's dma-buf. If the export fails it continues without waiting. Present timing comes from `VK_KHR_present_wait` with an acquire-based fallback. The only GPU it has ever run on is the Deck's AMD Van Gogh under RADV. See proposal.md for the verified/assumed split and motivation.

Constraints:
- Opaque-fd external memory only imports on the same device (matching device and driver UUIDs); a wrong choice is a failure, not a slowdown.
- The driver sees SteamVR only through `VRIPCResourceManager` and the direct-mode callbacks; it has no Vulkan device of its own and no known way to ask SteamVR which GPU it uses.
- Test hardware beyond the Deck is not owned; volunteers' reports are an input, so the report format is part of the design.

## Goals / Non-Goals

**Goals:**
- No silent failure modes: every GPU either works with a named strategy or says why not.
- Support decisions made from data (reports and sweep captures), not guesses.
- The AMD path stays byte-for-byte as it is, apart from logging.

**Non-Goals:**
- Cross-GPU copies, other platforms, or fixing vendor-specific SteamVR bugs.

## Decisions

### 1. GPU selection by identity, with a fallback chain

Order of preference: (a) SteamVR's GPU identity if it exposes one (candidates to check in the first experiment: SteamVR's own logs and settings, the compositor's startup lines, the IPC objects); (b) the device the Vulkan loader enumerates first without any offload variables, which is also what Steam launches SteamVR on by default; (c) the user's explicit choice. Every choice is logged with how it was made. The presenter then verifies the choice by attempting the first import and reports a mismatch as a mismatch.

Why: a wrong guess fails at import, so the verification step turns a silent bad choice into a message. Alternatives: probe-by-import alone (cheap but may "succeed" on a same-vendor wrong device, which the UUID check prevents), or asking the driver (no known API).

### 2. A defined wait when no sync file exists

Keep the fence wait wherever the export works. Where it fails, wait a bounded fallback: hold the frame until its age since Present is at least a configured time, default taken from the measured time-to-fence-signal on the AMD path (the p99 of writer completion, measured in the first experiment), plus a margin. The fallback is slower by a frame fraction but is defined and visible. If SteamVR turns out to offer a semaphore the driver can pass, that is preferred and is a variant of the same requirement.

Why a fixed delay is acceptable: ALVR's Linux driver does the same for the one writer it cannot order. The AMD machine is the control: it has both paths, so the fallback can be forced there (a debug switch that pretends the export failed) and scored against the fence path with the same sweep capture before any non-AMD hardware is involved.

### 3. One report, built from tools that already exist

`tools/gpu_report.sh` runs the presenter in a report mode (device listing, extension support, import attempt, active strategies) and the existing sim-pose sweep capture with `find_bad_frames.py`, then prints a paste-ready text report. The presenter gains no new subsystem; it exposes its decisions. The report excludes images and personal data by construction (counts and names only).

### 4. Vendor-neutral tooling

Scripts stop assuming `card1`, `DP-1` and the amdgpu counter: the glasses' connector is found from the DRM mode list (as `doctor.sh` already does for mode checks), and GPU load is read from amdgpu sysfs, `nvidia-smi` or `intel_gpu_top` if present, skipped otherwise.

### 5. The supported table is data-driven

A GPU is "supported" only when its report passes: import works, a strategy is defined, and the sweep capture's bad-frame count meets the same threshold used for the Deck (documented in the table). Failing reports are recorded too.

## Risks / Trade-offs

- [No NVIDIA/Intel/hybrid hardware to test on] → The forced-fallback switch on the Deck tests the code path; the report format lets volunteers test the hardware; tasks that need hardware are separate and may end in "not tested yet" without blocking the rest.
- [The fallback delay adds latency or still tears] → Judged by the sweep capture against the fence path on the Deck; the default is tuned from that, and the delay is configurable.
- [Identity matching has no data source] → The fallback chain ends in a manual override, and a mismatch is reported instead of presented as garbage.
- [NVIDIA's proprietary stack behaves differently in more ways (no present_wait, different swapchain behaviour)] → The report records vblank source and extension support; the existing acquire fallback covers missing present wait.
- [Deck/AMD/Wayland specific] → The Deck stays the reference; nothing in the AMD path changes; the compatibility table says what is verified.

## Migration Plan

No migration. The new behaviour is logging and a defined fallback; the AMD path is unchanged. The debug switch that forces the fallback is off by default. Rollback is reverting the commits.

## Open Questions

- The exact threshold for "supported" is set from the Deck's measured bad-frame range in the first report; it does not change the structure of the work.
