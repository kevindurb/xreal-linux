# Reverse-engineering helpers for the vendor SDK libraries

Small scripts used to read the ARM64 libraries of XREAL's ControlGlasses app (`libnr_service.so`, `libnr_api.so`, `libnr_glasses_api.so`). They only read files;
nothing here talks to the glasses. **All addresses in `docs/` refer to one build: ControlGlasses `3.1.0.20251118115716`**, so use that exact APK.

## Setup

    curl -LO https://public-resource.xreal.com/download/XREALSDK_Release_3.1.0.20251124/ControlGlasses-3.1.0.20251118115716-release.apk   # public, about 281 MB
    mkdir -p cg31/x && unzip -q ControlGlasses-3.1.0.20251118115716-release.apk 'lib/arm64-v8a/*' -d cg31/x
    python3 -m venv v && v/bin/pip install capstone pyelftools
    # run the scripts from the directory that contains cg31/ (keep the APK, the libraries and the venv out of the repo)

## Scripts (run with `v/bin/python tools/re/<script> <library> ...`)

| Script | What it does | Example |
|---|---|---|
| `adis.py LIB START SIZE` | disassemble `SIZE` bytes at virtual address `START` (hex), with PLT call names | `adis.py cg31/x/lib/arm64-v8a/libnr_service.so 0x1900354 200` |
| `camseq_scan.py LIB [IDS...]` | find `mov w?, #id` loads of the camera request ids (or the hex ids you pass): the sites that build or send those requests | `camseq_scan.py LIB 0x2748` |
| `cam_wrappers.py LIB` | find the functions that log "Call NRGrayscaleCamera... start" and where each starts | |
| `blcallers.py LIB ADDR...` | direct `bl`/`b` callers of the given function addresses | `blcallers.py LIB 0x1900354` |
| `rtti.py LIB MANGLED...` | virtual table (and method addresses) of a class from its RTTI name | `rtti.py LIB 13ImpGrayCamera` |
| `strrefs.py LIB LO HI` | strings referenced by code between two addresses | |
| `xref.py LIB ADDR...` | code references (adrp+add) to data addresses | |
| `callers.py LIB SYMBOL...` | callers of imported (PLT) symbols | |
| `idmap.py ADDR` | map a request class's serialiser method to its message id (path to the 3.1.0 service library is hard-coded) | |

## What these were used to find (see `docs/xreal-link-messages.md` section 13.5)

- The service has one wrapper function per grayscale-camera request (Create `0x18fb7e8`, PixelFormat `0x18fc49c`, ImageResolution `0x18fd128`, AutoExposureType `0x18fddb4`,
  ExposureTime `0x18fea40`, Gain `0x18ff6cc`, Start `0x1900354`, Stop `0x1900bf8`), each taking one integer, reached through the virtual table of
  `DriverInterface<NRGrayscaleCameraInterface>` (`0x2377e30`). `ImpGrayCamera`'s virtual table is at `0x2377d50` (21 slots, methods around `0x123a2f0` to `0x123de30`).
- **Open:** the integer values the service passes to the `InitSet*` wrappers. Next step: find the callers of `ImpGrayCamera`'s methods (through the vtable) or the
  `GrayscaleCameraProvider` setup code and read the `mov w1, #imm` before each call.
