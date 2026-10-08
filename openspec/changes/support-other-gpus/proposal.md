# Proposal

## Why

The presenter has only ever run on one GPU: an AMD Van Gogh (Steam Deck) under Mesa RADV. Most PC gaming hardware is NVIDIA, with Intel Arc and AMD desktop parts after that, and many laptops have two GPUs. The project's goal is to work on any Linux gaming PC, and today a non-AMD owner would get either no image or an unsynchronised one without being told why. This change makes the GPU handling explicit, tested and reportable, and records what actually works on each vendor.

## What We Know and What We Assume

Verified on the Deck (AMD/RADV):
- SteamVR's swap images import into the presenter as opaque-fd external memory with SteamVR's exact image parameters, on the same GPU.
- The presenter finds SteamVR's pending writes by exporting a sync file from each image's dma-buf (`DMA_BUF_IOCTL_EXPORT_SYNC_FILE`): about a fifth of frames had writes still pending, and waiting on them is what the presenter does.
- `VK_KHR_present_wait` and `present_id` work and give real vblank times; when they time out the presenter falls back to an acquire estimate and retries.

Read from the code, not yet exercised on any other GPU:
- The presenter picks the first Vulkan device that can present to its window. It does not check that this is the GPU SteamVR renders on.
- If the sync-file export fails, the presenter silently does not wait for the writer at all; the 25 ms CPU wait only covers the case where the GPU cannot import a sync file.
- The measurement tooling reads `amdgpu` sysfs counters and the Deck's `card1`/`DP-1` names.

Assumed, with the experiment that settles each:

| Assumption | Experiment |
|---|---|
| On NVIDIA (proprietary) the swap images import as opaque-fd memory, as ALVR's Linux driver does | Run the presenter's import path on an NVIDIA machine against SteamVR; check the import log and the test pattern |
| The fd SteamVR gives on NVIDIA is not a dma-buf, so the sync-file export fails and there is no write sync today | Run the export on an NVIDIA machine and log `fence_supported`; capture sweeps for torn or half-drawn frames |
| An alternative wait (a fixed delay after Present, as ALVR does for a writer it cannot order, or a Vulkan external semaphore SteamVR offers) is good enough where the export is missing | Compare sweep captures on the same machine with each strategy |
| On a laptop with two GPUs the glasses' output belongs to one GPU while SteamVR renders on the other, and the "first presentable device" guess picks wrong | Test on a hybrid laptop with the glasses on each output |
| The driver can learn which GPU SteamVR renders on (or the presenter can match by device UUID/LUID) | Look at what SteamVR's IPC and the driver API expose; test matching on two GPUs |
| Intel Arc under Mesa ANV presents and imports the same way; its known SteamVR flicker (SteamVR-for-Linux #932) is separate from ours | Run on an Arc machine if one is available |
| `present_wait` is missing or slower on some stacks, and the fallback is good enough | Run on each machine and read the log |

## What Changes

- Choose the presenter's GPU deliberately: the one SteamVR renders on, matched by device identity, with a clear message and an override when the match is impossible.
- Define the synchronisation strategy per situation: sync-file wait where the export works, a documented fallback where it does not, and a log line and report entry saying which one is in use. The silent no-wait case goes away.
- A GPU compatibility report, runnable and shareable by users, that lists the GPU, driver, which Vulkan features are present, which import and sync paths worked, and a short capture-based verdict, so support for a new GPU is tested by data and not by guessing.
- Make the setup checks and measurement tooling vendor-neutral (output names, GPU load counters).
- Record, per tested GPU, what works in a compatibility table in the docs. A GPU is called supported only with a passing report.

## Capabilities

### New Capabilities
- `gpu-selection`: which GPU the presenter uses and how it matches SteamVR's.
- `gpu-synchronisation`: how the presenter waits for SteamVR's writes on each kind of GPU and how it says which method is active.
- `gpu-compatibility-report`: the shareable report and the supported-hardware table.

### Modified Capabilities
- `presenter-display`: "Import SteamVR's textures" (same GPU is determined, not assumed) and "Do not show a frame SteamVR may still be drawing" (the fallback wait, no silent skip).

## Impact

- `presenter/src/main.rs` (device selection, the wait path), a possible driver addition if SteamVR's GPU must be reported from the driver side, and `tools/` (vendor-neutral GPU load and output detection).
- New docs: the compatibility table and how to submit a report.
- Needs test hardware that this project does not own yet (NVIDIA, Intel, a hybrid laptop); reports from volunteers are an input.
- No change for the verified AMD path beyond logging which strategy it uses.

## Non-goals

- Making every GPU work. A report that shows a stack does not work is a valid outcome and is recorded.
- Supporting a presenter and SteamVR on different GPUs (cross-device copies).
- Windows or macOS, or non-Vulkan paths.
- Fixing SteamVR's own bugs on other vendors (for example the Arc flicker), only documenting them.
- A GUI for the report; it is a command whose output a user pastes into an issue.
