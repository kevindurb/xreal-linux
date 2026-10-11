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

        def boom(url, timeout=0):
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

        def handler(url, timeout=0):
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

    def test_a_checksum_mismatch_installs_nothing(self):
        p = fake_plugin()
        sums = f"{'0' * 64}  {main.APPIMAGE_ASSET}\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *exc):
                self.close()

        def handler(url, timeout=0):
            return Response(sums.encode() if url.endswith(main.CHECKSUM_ASSET) else b"a downloaded appimage")

        out = self._with_urlopen(handler, lambda: p._install_sync("v0.1.0"))
        self.assertFalse(out["ok"])
        self.assertEqual(out["kind"], "checksum", "the published checksum is verified before anything is placed")


if __name__ == "__main__":
    unittest.main()
