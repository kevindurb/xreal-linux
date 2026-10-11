"""XREAL game-mode control: a Decky plugin that drives the installed xreal-linux runtime.

The panel is a thin translation of `systemctl --user` and the runtime's own CLI, run **as the session user** (the Decky
backend runs with more privilege). It never speaks the glasses' protocol itself, so the project rule that only
allowlisted requests reach the glasses is untouched. See openspec/changes/add-decky-xreal-control.
"""

import asyncio
import hashlib
import json
import os
import pwd
import shutil
import ssl
import subprocess
import tempfile
import urllib.parse
import urllib.request

try:  # decky-loader provides this at runtime; the pure helpers are importable (and tested) without it.
    import decky
except Exception:  # pragma: no cover - only when running outside decky-loader
    decky = None

SOCKET = "xreal-linux.socket"
SERVICE = "xreal-linux.service"
HEADLOOK = "xreal-headlook.service"
VALID_MODES = ("off", "head_mouse", "steamvr")

# The release this plugin installs from. The plugin's own version maps to the release tag, so the runtime and the
# plugin always come from the same build.
REPO = "kevindurb/xreal-linux"
APPIMAGE_ASSET = "xreal-linux-x86_64.AppImage"  # the release asset we download
INSTALLED_APPIMAGE = "xreal-linux.AppImage"  # the name setup places in $XDG_DATA_HOME/xreal-linux
CHECKSUM_ASSET = "SHA256SUMS"
# The structured report must carry these for the panel to render; an older runtime lacks them ("runtime too old").
REQUIRED_REPORT_KEYS = ("device", "units")


# ---- pure helpers (unit-tested without decky-loader) -------------------------------------------------------------------


def runtime_dir(home: str) -> str:
    return os.path.join(home, ".local", "share", "xreal-linux")


def appimage_path(home: str) -> str:
    return os.path.join(runtime_dir(home), INSTALLED_APPIMAGE)


def version_file(home: str) -> str:
    return os.path.join(runtime_dir(home), "VERSION")


def parse_version(text: str):
    """`commit <hash>` / `built <seconds>` from a VERSION file, or None."""
    commit = None
    built = 0
    for line in (text or "").splitlines():
        if line.startswith("commit "):
            commit = line[len("commit "):].strip() or None
        elif line.startswith("built "):
            try:
                built = int(line[len("built "):].strip())
            except ValueError:
                built = 0
    return {"commit": commit, "built": built} if commit else None


def session_env(uid: int, home: str, user: str = "", wayland: str = "", base=None) -> dict:
    """The environment a user-session command needs, whatever uid the backend runs as."""
    env = dict(os.environ if base is None else base)
    env["HOME"] = home
    if user:
        env["USER"] = user
        env["LOGNAME"] = user
    env["XDG_RUNTIME_DIR"] = f"/run/user/{uid}"
    env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path=/run/user/{uid}/bus"
    if wayland:
        env["WAYLAND_DISPLAY"] = wayland
    # Do not leak the backend's own data/config roots into the user's session.
    for key in ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"):
        env.pop(key, None)
    # Decky's frozen backend (PyInstaller) points LD_LIBRARY_PATH at its bundled libraries; a child like
    # systemctl then loads the wrong libcrypto and fails before doing anything. Hand children the
    # pre-loader value, or none.
    if "LD_LIBRARY_PATH_ORIG" in env:
        env["LD_LIBRARY_PATH"] = env.pop("LD_LIBRARY_PATH_ORIG")
    else:
        env.pop("LD_LIBRARY_PATH", None)
    env.pop("LD_PRELOAD", None)
    return env


def as_user_argv(argv, uid: int, gid=None, euid=None):
    """Prefix the privilege drop when running as root; nothing is added in a normal user session."""
    argv = list(argv)
    euid = os.geteuid() if euid is None else euid
    if euid != 0:
        return argv
    prefix = ["setpriv", f"--reuid={uid}"]
    if gid is not None:
        prefix.append(f"--regid={gid}")
    prefix += ["--init-groups", "--"]
    return prefix + argv


def detect_wayland(uid: int):
    """The compositor's Wayland socket for the user, if one exists (`wayland-0`, `gamescope-0`, ...)."""
    directory = f"/run/user/{uid}"
    try:
        names = sorted(n for n in os.listdir(directory) if n.startswith("wayland-") and not n.endswith(".lock"))
    except OSError:
        return None
    return names[0] if names else None


def expected_tag(version: str) -> str:
    version = (version or "0.0.0").strip()
    return version if version.startswith("v") else "v" + version


def release_url(tag: str, asset: str) -> str:
    return f"https://github.com/{REPO}/releases/download/{tag}/{urllib.parse.quote(asset)}"


_CA_CONTEXT = None


def ssl_context():
    """The TLS context for release downloads, with a CA bundle that actually exists.

    Decky's backend runs under a frozen Python that bundles `certifi` but has no default CA path, so
    urllib's implicit context cannot verify github.com and every fetch fails before it starts. Use
    certifi's bundle when it is importable, then the host's, then the interpreter default.
    """
    global _CA_CONTEXT
    if _CA_CONTEXT is not None:
        return _CA_CONTEXT
    candidates = []
    try:
        import certifi

        candidates.append(certifi.where())
    except Exception:
        pass
    candidates += ["/etc/ssl/certs/ca-certificates.crt", "/etc/pki/tls/certs/ca-bundle.crt", "/etc/ssl/cert.pem"]
    for path in candidates:
        if path and os.path.exists(path):
            try:
                _CA_CONTEXT = ssl.create_default_context(cafile=path)
                return _CA_CONTEXT
            except Exception:
                pass
    _CA_CONTEXT = ssl.create_default_context()
    return _CA_CONTEXT


def checksum_for(name: str, sums_text: str):
    """The hex sha256 for `name` in a `sha256sum`-style file, or None."""
    for line in (sums_text or "").splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[-1].lstrip("*") == name:
            return parts[0].lower()
    return None


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def report_is_usable(report) -> bool:
    """Whether a structured report has the fields this plugin needs (older runtimes do not)."""
    return isinstance(report, dict) and all(k in report for k in REQUIRED_REPORT_KEYS)


def first_problem(setup_output: str) -> str:
    """The first line under `Problems:` in the runtime's setup output, so the panel can say why setup was unhappy."""
    lines = [l.strip() for l in (setup_output or "").splitlines()]
    for i, line in enumerate(lines):
        if line == "Problems:":
            after = [l for l in lines[i + 1:] if l]
            return after[0] if after else "setup reported problems"
    return ""


# ---- the plugin ----------------------------------------------------------------------------------------------------------


class Plugin:
    # ---- process plumbing ----

    def _user_defaults(self):
        user = getattr(decky, "DECKY_USER", None) or os.environ.get("DECKY_USER") or "deck"
        home = getattr(decky, "DECKY_USER_HOME", None) or f"/home/{user}"
        try:
            pw = pwd.getpwnam(user)
            uid, gid = pw.pw_uid, pw.pw_gid
        except KeyError:
            st = os.stat(home)
            uid, gid = st.st_uid, st.st_gid
        return user, home, uid, gid

    def _run_user_sync(self, argv, timeout=30):
        env = session_env(self.uid, self.home, self.user, self.wayland)
        full = as_user_argv(argv, self.uid, self.gid)
        try:
            return subprocess.run(full, env=env, capture_output=True, text=True, timeout=timeout)
        except FileNotFoundError as e:
            return subprocess.CompletedProcess(full, 127, "", str(e))
        except subprocess.TimeoutExpired as e:
            out = e.stdout.decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
            return subprocess.CompletedProcess(full, 124, out, "timed out")

    async def _run_user(self, argv, timeout=30):
        return await asyncio.to_thread(self._run_user_sync, argv, timeout)

    async def _systemctl(self, *args):
        return await self._run_user(["systemctl", "--user", *args], timeout=20)

    async def _unit(self, verb, unit):
        p = await self._systemctl(verb, unit)
        text = (p.stdout or "").strip() or (p.stderr or "").strip()
        return text.splitlines()[0].strip() if text else "unknown"

    async def _unit_pair(self, unit):
        return {"enabled": await self._unit("is-enabled", unit), "active": await self._unit("is-active", unit)}

    async def _steamvr_running(self):
        """A live session: the socket-activated service is up, or a SteamVR server process is running."""
        if await self._unit("is-active", SERVICE) == "active":
            return True
        p = await self._run_user(["pgrep", "-x", "vrserver"], timeout=5)
        return p.returncode == 0

    async def _report(self):
        app = appimage_path(self.home)
        p = await self._run_user([app, "status", "--json"], timeout=25)
        text = (p.stdout or "").strip()
        data = None
        error = None
        if text:
            try:
                data = json.loads(text)
            except json.JSONDecodeError:
                error = "the runtime's report was not JSON"
        else:
            error = (p.stderr or "the runtime produced no report").strip()
        return {"data": data, "exit": p.returncode, "error": error}

    # ---- backend API ----

    async def get_state(self):
        installed = os.path.isfile(appimage_path(self.home))
        return {
            "user": self.user,
            "units": {
                "socket": await self._unit_pair(SOCKET),
                "service": {"active": await self._unit("is-active", SERVICE)},
                "headlook": await self._unit_pair(HEADLOOK),
            },
            "runtime": {
                "installed": installed,
                "version": self._read_version(),
                "path": appimage_path(self.home),
            },
            "report": await self._report() if installed else None,
            "expected_version": self.version,
        }

    def _read_version(self):
        path = version_file(self.home)
        try:
            with open(path) as f:
                return parse_version(f.read())
        except OSError:
            return None

    async def set_mode(self, mode, confirm=False):
        if mode not in VALID_MODES:
            return {"ok": False, "error": f"unknown mode '{mode}'"}
        if mode != "steamvr" and not confirm and await self._steamvr_running():
            return {
                "ok": False,
                "needs_confirmation": True,
                "reason": "A SteamVR session is running. Switching away would stop the presenter and take the glasses' display away mid-session.",
            }
        plan = {
            "off": [("disable", HEADLOOK), ("disable", SOCKET)],
            "head_mouse": [("enable", HEADLOOK), ("disable", SOCKET)],
            "steamvr": [("enable", SOCKET), ("disable", HEADLOOK)],
        }[mode]
        results = []
        ok = True
        for verb, unit in plan:
            p = await self._systemctl(verb, "--now", unit)
            good = p.returncode == 0
            ok = ok and good
            results.append({"unit": unit, "action": f"{verb} --now", "ok": good, "output": ((p.stderr or "") + (p.stdout or "")).strip()[:300]})
        state = await self.get_state()
        state.update({"ok": ok, "results": results})
        if not ok:
            bad = next((r for r in results if not r["ok"]), None)
            if bad and bad["output"]:
                state["error"] = f"{bad['action']} {bad['unit']} failed: {bad['output']}"
            elif bad:
                state["error"] = f"{bad['action']} {bad['unit']} failed"
            else:
                state["error"] = "the mode change did not fully apply"
        return state

    async def install_runtime(self):
        """Download and install the runtime for this plugin's release tag, verifying the published checksum first."""
        result = await asyncio.to_thread(self._install_sync, expected_tag(self.version))
        if not result.get("ok"):
            self._log("warning", f"install of {expected_tag(self.version)} did not complete: {result.get('error')}")
        return result

    def _log(self, level, message):
        """Log through decky when it is there (a failed install is then diagnosable from the plugin's log file)."""
        if decky is not None:
            try:
                getattr(decky.logger, level)(message)
            except Exception:
                pass

    def _install_sync(self, tag):
        app_url = release_url(tag, APPIMAGE_ASSET)
        sums_url = release_url(tag, CHECKSUM_ASSET)
        context = ssl_context()
        try:
            with urllib.request.urlopen(sums_url, timeout=30, context=context) as r:
                sums = r.read().decode("utf-8", "replace")
        except Exception as e:
            return {"ok": False, "kind": "mismatch", "error": f"no {CHECKSUM_ASSET} for {tag} ({e}); installing nothing"}
        want = checksum_for(APPIMAGE_ASSET, sums)
        if not want:
            return {"ok": False, "kind": "mismatch", "error": f"{APPIMAGE_ASSET} is not listed for {tag}; installing nothing"}
        tmp, why = self._download_verified(app_url, want, context)
        if tmp is None:
            kind = "checksum" if why.startswith("checksum") else "download"
            return {"ok": False, "kind": kind, "error": f"{why}; nothing was installed"}
        directory = runtime_dir(self.home)
        os.makedirs(directory, exist_ok=True)
        dest = appimage_path(self.home)
        shutil.move(tmp, dest)
        os.chmod(dest, 0o755)
        # The backend runs as root with the `_root` flag; `setup` runs as the session user, so the runtime directory and the
        # AppImage must belong to that user or setup cannot write the driver beside it.
        self._own_as_user(directory)
        self._own_as_user(dest)
        # `--no-enable`: place the files and every user unit but enable none, so the panel's switch is what turns a mode on.
        p = self._run_user_sync([dest, "setup", "--yes", "--no-enable"], timeout=900)
        output = ((p.stdout or "") + (p.stderr or "")).strip()
        result = {"ok": p.returncode == 0, "kind": "installed", "tag": tag, "output": output[-2000:]}
        if p.returncode != 0:
            result["error"] = first_problem(output) or f"setup exited with {p.returncode}"
        return result

    def _download_verified(self, app_url, want, context, attempts=3):
        """Fetch the asset into a temp file, retrying a flaky transfer (a truncated one would otherwise look like a bad
        checksum). Returns `(path, "")` only for a file whose sha256 is `want`, else `(None, why)` after the last attempt."""
        why = "download failed"
        for _ in range(attempts):
            tmp = tempfile.NamedTemporaryFile(prefix="xreal-", suffix=".AppImage", delete=False)
            tmp.close()
            try:
                with urllib.request.urlopen(app_url, timeout=180, context=context) as r, open(tmp.name, "wb") as f:
                    shutil.copyfileobj(r, f)
            except Exception as e:
                os.unlink(tmp.name)
                why = f"download failed ({e})"
                continue
            got = sha256_file(tmp.name)
            if got == want:
                return tmp.name, ""
            os.unlink(tmp.name)
            why = f"checksum mismatch (got {got}, expected {want})"
        return None, why

    def _own_as_user(self, path):
        """Hand a path the root backend created back to the session user, who owns the runtime (no-op in a user session)."""
        if os.geteuid() != 0:
            return
        try:
            os.chown(path, self.uid, self.gid)
        except OSError:
            pass

    # ---- lifecycle ----

    async def _main(self):
        self.user, self.home, self.uid, self.gid = self._user_defaults()
        self.wayland = detect_wayland(self.uid)
        self.version = getattr(decky, "DECKY_PLUGIN_VERSION", None) or "0.0.0"
        if decky is not None:
            decky.logger.info(f"XREAL control ready: user {self.user} (uid {self.uid}), plugin {self.version}")

    async def _unload(self):
        pass

    async def _uninstall(self):
        # Never touch the installed runtime here: it is the user's, and uninstalling the plugin must not remove it.
        pass

    async def _migration(self):
        pass
