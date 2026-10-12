"""Unit tests for the parts of decky/main.py that do not need decky-loader: the run-as-session-user helper's
argument and environment construction, the report/version helpers and the release-tag mapping.

Run: python3 -m unittest discover -s tests -v   (from the decky/ directory)
"""

import asyncio
import hashlib
import importlib.util
import io
import json
import os
import ssl
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
spec = importlib.util.spec_from_file_location("xreal_main", os.path.join(os.path.dirname(HERE), "main.py"))
main = importlib.util.module_from_spec(spec)
spec.loader.exec_module(main)


class SessionEnv(unittest.TestCase):
    def test_sets_the_session_variables_and_clears_the_backends_own_roots(self):
        env = main.session_env(1000, "/home/deck", "deck", "wayland-0", base={
            "PATH": "/usr/bin",
            "XDG_RUNTIME_DIR": "/run/user/0",
            "XDG_DATA_HOME": "/root/.local/share",
            "XDG_CONFIG_HOME": "/root/.config",
        })
        self.assertEqual(env["HOME"], "/home/deck")
        self.assertEqual(env["USER"], "deck")
        self.assertEqual(env["LOGNAME"], "deck")
        self.assertEqual(env["XDG_RUNTIME_DIR"], "/run/user/1000")
        self.assertEqual(env["DBUS_SESSION_BUS_ADDRESS"], "unix:path=/run/user/1000/bus")
        self.assertEqual(env["WAYLAND_DISPLAY"], "wayland-0")
        self.assertEqual(env["PATH"], "/usr/bin", "the rest of the environment is kept")
        self.assertNotIn("XDG_DATA_HOME", env)
        self.assertNotIn("XDG_CONFIG_HOME", env)

    def test_no_wayland_socket_leaves_the_variable_unset(self):
        env = main.session_env(1000, "/home/deck", "deck", "")
        self.assertNotIn("WAYLAND_DISPLAY", env)

    def test_a_frozen_backends_library_path_does_not_reach_children(self):
        env = main.session_env(1000, "/home/deck", "deck", "", base={
            "LD_LIBRARY_PATH": "/tmp/_MEI123",
            "LD_PRELOAD": "/tmp/_MEI123/libx.so",
        })
        self.assertNotIn("LD_LIBRARY_PATH", env, "PyInstaller's path would make systemctl load the wrong libcrypto")
        self.assertNotIn("LD_PRELOAD", env)

    def test_the_pre_loader_library_path_is_restored_when_the_loader_saved_one(self):
        env = main.session_env(1000, "/home/deck", "deck", "", base={
            "LD_LIBRARY_PATH": "/tmp/_MEI123",
            "LD_LIBRARY_PATH_ORIG": "/opt/mine/lib",
        })
        self.assertEqual(env["LD_LIBRARY_PATH"], "/opt/mine/lib", "a path that was there before the loader is kept")
        self.assertNotIn("LD_LIBRARY_PATH_ORIG", env)


class AsUserArgv(unittest.TestCase):
    def test_drops_privilege_only_when_root(self):
        argv = ["systemctl", "--user", "is-active", main.HEADLOOK]
        self.assertEqual(main.as_user_argv(argv, 1000, 1000, euid=0), [
            "setpriv", "--reuid=1000", "--regid=1000", "--init-groups", "--",
            "systemctl", "--user", "is-active", main.HEADLOOK,
        ])
        self.assertEqual(main.as_user_argv(argv, 1000, 1000, euid=1000), argv, "a normal session needs no prefix")

    def test_without_a_gid_it_still_drops_the_uid(self):
        self.assertEqual(main.as_user_argv([":", "x"], 1000, None, euid=0)[:2], ["setpriv", "--reuid=1000"])


class ReportHelpers(unittest.TestCase):
    def test_a_report_is_usable_only_with_the_required_keys(self):
        self.assertTrue(main.report_is_usable({"device": {}, "units": {}}))
        self.assertFalse(main.report_is_usable({"device": {}}), "an older runtime's report is too old")
        self.assertFalse(main.report_is_usable(None))
        self.assertFalse(main.report_is_usable("not a dict"))

    def test_version_file_parsing(self):
        self.assertEqual(main.parse_version("commit abc123\nbuilt 1700000000\n"),
                         {"commit": "abc123", "built": 1700000000})
        self.assertIsNone(main.parse_version(""))
        self.assertIsNone(main.parse_version("built 1700000000\n"), "a commit is required")

    def test_first_problem_finds_the_reason_setup_reported(self):
        output = "Files\n  placed /x\nProblems:\n  - SteamVR not installed: run setup again\nResult: 1 ok, 1 failure\n"
        self.assertEqual(main.first_problem(output), "- SteamVR not installed: run setup again")
        self.assertEqual(main.first_problem("Problems:\n"), "setup reported problems")
        self.assertEqual(main.first_problem("all good"), "", "no problems section means no reason to give")


class ReleaseHelpers(unittest.TestCase):
    def test_the_plugin_version_maps_to_a_release_tag(self):
        self.assertEqual(main.expected_tag("0.1.0"), "v0.1.0")
        self.assertEqual(main.expected_tag("v0.1.0"), "v0.1.0")

    def test_checksum_lookup_handles_the_star_form_and_ignores_others(self):
        sums = "\n".join([
            "aaaa  other.AppImage",
            "bbbb  *" + main.APPIMAGE_ASSET,
        ])
        self.assertEqual(main.checksum_for(main.APPIMAGE_ASSET, sums), "bbbb")
        self.assertIsNone(main.checksum_for("missing", sums))
        self.assertIsNone(main.checksum_for(main.APPIMAGE_ASSET, ""))

    def test_release_url_points_at_the_repo_tag_asset(self):
        self.assertEqual(main.release_url("v0.1.0", main.APPIMAGE_ASSET),
                         "https://github.com/kevindurb/xreal-linux/releases/download/v0.1.0/" + main.APPIMAGE_ASSET)


class Modes(unittest.TestCase):
    def test_every_mode_is_known(self):
        self.assertEqual(set(main.VALID_MODES), {"off", "head_mouse", "steamvr"})


def fake_plugin(version="0.1.0"):
    p = main.Plugin()
    p.user, p.home, p.uid, p.gid = "deck", "/home/deck", 1000, 1000
    p.wayland = None
    p.version = version
    return p


class SetMode(unittest.TestCase):
    """set_mode must send the exact `systemctl --user` command set for each mode, through the helper."""

    def _run(self, mode, steamvr=False, confirm=False):
        p = fake_plugin()
        seen = []

        async def systemctl(*args):
            seen.append(args)
            return subprocess.CompletedProcess(["systemctl", "--user", *args], 0, "", "")

        async def running():
            return steamvr

        async def state():
            return {"user": "deck", "units": {}, "runtime": {}, "report": None, "expected_version": p.version}

        p._systemctl = systemctl
        p._steamvr_running = running
        p.get_state = state
        return asyncio.run(p.set_mode(mode, confirm)), seen

    def test_each_mode_sends_the_exact_command_set(self):
        result, seen = self._run("off")
        self.assertEqual(seen, [("disable", "--now", main.HEADLOOK), ("disable", "--now", main.SOCKET)])
        self.assertTrue(result["ok"])
        result, seen = self._run("head_mouse")
        self.assertEqual(seen, [("enable", "--now", main.HEADLOOK), ("disable", "--now", main.SOCKET)])
        result, seen = self._run("steamvr")
        self.assertEqual(seen, [("enable", "--now", main.SOCKET), ("disable", "--now", main.HEADLOOK)])

    def test_the_units_are_never_both_enabled(self):
        for mode in main.VALID_MODES:
            _, seen = self._run(mode)
            verbs = {a[2]: a[0] for a in seen}
            self.assertEqual(sorted(verbs), [main.HEADLOOK, main.SOCKET], f"{mode} states both units explicitly")
            self.assertFalse(all(v == "enable" for v in verbs.values()), f"{mode} must not enable both")
            enabled = [u for u, v in verbs.items() if v == "enable"]
            expected = {"off": [], "head_mouse": [main.HEADLOOK], "steamvr": [main.SOCKET]}[mode]
            self.assertEqual(sorted(enabled), sorted(expected), f"{mode} enables exactly the right unit")

    def test_leaving_a_live_session_needs_confirmation_but_targeting_steamvr_does_not(self):
        result, seen = self._run("off", steamvr=True)
        self.assertTrue(result.get("needs_confirmation"), "OFF during a live session asks first")
        self.assertEqual(seen, [], "nothing runs before confirmation")
        result, seen = self._run("off", steamvr=True, confirm=True)
        self.assertFalse(result.get("needs_confirmation", False))
        self.assertEqual(len(seen), 2, "with confirmation it proceeds")
        result, _ = self._run("head_mouse", steamvr=True)
        self.assertTrue(result.get("needs_confirmation"))
        result, seen = self._run("steamvr", steamvr=True)
        self.assertFalse(result.get("needs_confirmation", False))
        self.assertEqual(len(seen), 2, "selecting SteamVR during a session is allowed")

    def test_an_unknown_mode_is_refused_without_running_anything(self):
        result, seen = self._run("nonsense")
        self.assertFalse(result["ok"])
        self.assertEqual(seen, [])

    def test_a_failed_unit_action_is_reported_with_its_reason(self):
        p = fake_plugin()

        async def systemctl(*args):
            if args[2] == main.SOCKET:
                return subprocess.CompletedProcess(["systemctl", *args], 1, "", "Failed to enable unit: No such file or directory")
            return subprocess.CompletedProcess(["systemctl", *args], 0, "", "")

        async def running():
            return False

        async def state():
            return {"user": "deck", "units": {}, "runtime": {}, "report": None, "expected_version": p.version}

        p._systemctl = systemctl
        p._steamvr_running = running
        p.get_state = state
        result = asyncio.run(p.set_mode("steamvr"))
        self.assertFalse(result["ok"])
        self.assertIn(main.SOCKET, result["error"], "the panel is told which action failed")
        self.assertIn("No such file", result["error"], "and why, not just 'The action failed.'")


class GetState(unittest.TestCase):
    def _state(self, installed, report):
        p = fake_plugin()

        async def unit_pair(_):
            return {"enabled": "disabled", "active": "inactive"}

        async def unit(*_):
            return "inactive"

        async def canned_report():
            return report

        p._unit_pair = unit_pair
        p._unit = unit
        p._report = canned_report
        p._read_version = lambda: {"commit": "abc123", "built": 1}
        original = main.os.path.isfile
        main.os.path.isfile = lambda path: installed
        try:
            return asyncio.run(p.get_state())
        finally:
            main.os.path.isfile = original

    def test_not_installed_reports_no_report(self):
        st = self._state(False, None)
        self.assertFalse(st["runtime"]["installed"])
        self.assertIsNone(st["report"])
        self.assertEqual(st["expected_version"], "0.1.0")

    def test_a_usable_report_is_exposed_to_the_front_end(self):
        st = self._state(True, {"data": {"device": {"connected": True}, "units": {}}, "exit": 0, "error": None})
        self.assertTrue(st["runtime"]["installed"])
        self.assertTrue(main.report_is_usable(st["report"]["data"]))

    def test_a_too_old_report_is_reported_as_unusable(self):
        st = self._state(True, {"data": {"device": {}}, "exit": 0, "error": None})
        self.assertFalse(main.report_is_usable(st["report"]["data"]), "a report without units is too old")


class Install(unittest.TestCase):
    def test_install_requests_the_plugins_own_tag(self):
        p = fake_plugin("0.2.0")
        asked = {}

        def fake(tag):
            asked["tag"] = tag
            return {"ok": True}

        p._install_sync = fake
        self.assertTrue(asyncio.run(p.install_runtime())["ok"])
        self.assertEqual(asked["tag"], "v0.2.0", "the plugin's version maps to the release tag")

    def _with_urlopen(self, handler, fn):
        import urllib.request
        original = urllib.request.urlopen
        urllib.request.urlopen = handler
        try:
            return fn()
        finally:
            urllib.request.urlopen = original

    def test_a_tag_without_a_checksum_installs_nothing(self):
        p = fake_plugin()

        def boom(url, timeout=0, **kwargs):
            raise OSError("404 Not Found")

        out = self._with_urlopen(boom, lambda: p._install_sync("v9.9.9"))
        self.assertFalse(out["ok"])
        self.assertEqual(out["kind"], "mismatch")

    def test_a_verified_artifact_installs_and_runs_setup_without_enabling(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"
        calls = []

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else payload)

        def run(argv, timeout=30):
            calls.append(argv)
            return subprocess.CompletedProcess(argv, 0, "done", "")

        with tempfile.TemporaryDirectory() as home:
            p.home = home
            dest = main.appimage_path(home)
            p._run_user_sync = run
            out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
            self.assertTrue(out["ok"], out)
            self.assertEqual(calls[0][1:], ["setup", "--yes", "--no-enable"],
                             "the panel's install provisions the units but enables none, so head mouse waits for the switch")
            self.assertTrue(os.path.isfile(dest))

    def test_the_backend_hands_the_runtime_to_the_session_user(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else payload)

        chowns = []
        real_geteuid, real_chown = os.geteuid, os.chown
        os.geteuid = lambda: 0  # the backend runs as root with the `_root` flag
        os.chown = lambda path, uid, gid: chowns.append((path, uid, gid))
        try:
            with tempfile.TemporaryDirectory() as home:
                p.home = home
                p._run_user_sync = lambda argv, timeout=30: subprocess.CompletedProcess(argv, 0, "ok", "")
                out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        finally:
            os.geteuid, os.chown = real_geteuid, real_chown
        self.assertTrue(out["ok"], out)
        owned = {os.path.basename(path.rstrip("/")): (uid, gid) for path, uid, gid in chowns}
        self.assertEqual(owned.get("xreal-linux"), (1000, 1000), "the runtime directory belongs to the session user")
        self.assertEqual(owned.get("xreal-linux.AppImage"), (1000, 1000), "so setup can write the driver beside it")

    def test_a_setup_that_reports_a_problem_is_not_ok_but_says_why(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else payload)

        def run(argv, timeout=30):
            out = "Files\n  placed /x\ndriver registration failed\nProblems:\n  - SteamVR is not installed\nResult: 1 ok, 1 failure\n"
            return subprocess.CompletedProcess(argv, 1, out, "")

        with tempfile.TemporaryDirectory() as home:
            p.home = home
            p._run_user_sync = run
            out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
            self.assertFalse(out["ok"], "setup's non-zero exit means the install finished with a problem")
            self.assertEqual(out["kind"], "installed")
            self.assertIn("SteamVR is not installed", out["error"], "the panel is told the reason, not 'unknown error'")

    def test_an_install_that_only_lacks_steamvr_is_still_successful(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else payload)

        problems = "Problems:\n  - /home/deck/.local/share/Steam/steamapps/common/SteamVR/bin/vrpathreg.sh not found: is SteamVR installed (Steam app 250820)?\n"

        def run(argv, timeout=30):
            return subprocess.CompletedProcess(argv, 1, problems, "")

        with tempfile.TemporaryDirectory() as home:
            p.home = home
            p._run_user_sync = run
            # setup placed the driver and the three units; only the SteamVR registration is missing.
            os.makedirs(os.path.dirname(os.path.join(main.runtime_dir(home), main.INSTALLED_DRIVER)), exist_ok=True)
            open(os.path.join(main.runtime_dir(home), main.INSTALLED_DRIVER), "w").close()
            os.makedirs(main.units_dir(home), exist_ok=True)
            for unit in (main.SOCKET, main.SERVICE, main.HEADLOOK):
                open(os.path.join(main.units_dir(home), unit), "w").close()
            out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        self.assertTrue(out["ok"], out)
        self.assertNotIn("error", out, "a missing SteamVR is not an install failure")
        self.assertIn("vrpathreg", out["note"], "but the reason is still reported")

    def test_an_install_that_placed_nothing_reports_the_problem(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else payload)

        def run(argv, timeout=30):
            return subprocess.CompletedProcess(argv, 1, "Problems:\n  - cannot write the driver beside the app\n", "")

        with tempfile.TemporaryDirectory() as home:
            p.home = home
            p._run_user_sync = run
            out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        self.assertFalse(out["ok"], "setup that placed nothing is still a failure")
        self.assertIn("cannot write the driver", out["error"])

    def test_a_flaky_download_is_retried_until_the_checksum_matches(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"
        calls = {"app": 0}

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            if url.endswith(main.CHECKSUM_ASSET):
                return Response(sums.encode())
            calls["app"] += 1
            return Response(b"a truncated transfer" if calls["app"] < 2 else payload)

        with tempfile.TemporaryDirectory() as home:
            p.home = home
            p._run_user_sync = lambda argv, timeout=30: subprocess.CompletedProcess(argv, 0, "ok", "")
            out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        self.assertTrue(out["ok"], out)
        self.assertEqual(calls["app"], 2, "the second, complete transfer is the one that is used")

    def test_every_fetch_uses_an_explicit_ca_context(self):
        p = fake_plugin()
        payload = b"a verified appimage"
        digest = hashlib.sha256(payload).hexdigest()
        sums = f"{digest}  {main.APPIMAGE_ASSET}\n"
        seen = []

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            seen.append(kwargs.get("context"))
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else payload)

        with tempfile.TemporaryDirectory() as home:
            p.home = home
            p._run_user_sync = lambda argv, timeout=30: subprocess.CompletedProcess(argv, 0, "ok", "")
            out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        self.assertTrue(out["ok"], out)
        self.assertEqual(len(seen), 2, "the checksum file and the AppImage")
        self.assertTrue(all(isinstance(c, ssl.SSLContext) for c in seen),
                        "Decky's frozen backend has no default CA path, so each fetch must carry its own context")

    def test_a_checksum_mismatch_installs_nothing(self):
        p = fake_plugin()
        sums = f"{'0' * 64}  {main.APPIMAGE_ASSET}\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0, **kwargs):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else b"a downloaded appimage")

        out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        self.assertFalse(out["ok"])
        self.assertEqual(out["kind"], "checksum", "the published checksum is verified before anything is placed")


if __name__ == "__main__":
    unittest.main()
