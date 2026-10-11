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
        return state

    async def install_runtime(self):
        """Download and install the runtime for this plugin's release tag, verifying the published checksum first."""
        return await asyncio.to_thread(self._install_sync, expected_tag(self.version))

    def _install_sync(self, tag):
        app_url = release_url(tag, APPIMAGE_ASSET)
        sums_url = release_url(tag, CHECKSUM_ASSET)
        try:
            with urllib.request.urlopen(sums_url, timeout=30) as r:
                sums = r.read().decode("utf-8", "replace")
        except Exception as e:
            return {"ok": False, "kind": "mismatch", "error": f"no {CHECKSUM_ASSET} for {tag} ({e}); installing nothing"}
        want = checksum_for(APPIMAGE_ASSET, sums)
        if not want:
            return {"ok": False, "kind": "mismatch", "error": f"{APPIMAGE_ASSET} is not listed for {tag}; installing nothing"}
        tmp = tempfile.NamedTemporaryFile(prefix="xreal-", suffix=".AppImage", delete=False)
        tmp.close()
        try:
            with urllib.request.urlopen(app_url, timeout=180) as r, open(tmp.name, "wb") as f:
                shutil.copyfileobj(r, f)
        except Exception as e:
            os.unlink(tmp.name)
            return {"ok": False, "kind": "download", "error": str(e)}
        got = sha256_file(tmp.name)
        if got != want:
            os.unlink(tmp.name)
            return {"ok": False, "kind": "checksum", "error": f"checksum mismatch (got {got}, expected {want}); nothing was installed"}
        directory = runtime_dir(self.home)
        os.makedirs(directory, exist_ok=True)
        dest = appimage_path(self.home)
        shutil.move(tmp.name, dest)
        os.chmod(dest, 0o755)
        # `--no-enable`: place the files and every user unit but enable none, so the panel's switch is what turns a mode on.
        p = self._run_user_sync([dest, "setup", "--yes", "--no-enable"], timeout=900)
        return {
            "ok": p.returncode == 0,
            "kind": "installed",
            "tag": tag,
            "output": ((p.stdout or "") + (p.stderr or ""))[-2000:],
        }

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
