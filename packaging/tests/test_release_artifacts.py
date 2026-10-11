import importlib.util
import os
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import release

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, "decky"))
_spec = importlib.util.spec_from_file_location("xreal_main", os.path.join(ROOT, "decky", "main.py"))
main = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(main)


class Sums(unittest.TestCase):
    def test_sums_are_sha256sum_style_sorted_and_parse_with_the_plugin(self):
        with tempfile.TemporaryDirectory() as d:
            a = os.path.join(d, "xreal-linux-x86_64.AppImage")
            b = os.path.join(d, "xreal-control.zip")
            with open(a, "wb") as f:
                f.write(b"the app")
            with open(b, "wb") as f:
                f.write(b"the plugin")
            text = release.sums_text([b, a])
            self.assertEqual([line.split("  ")[1] for line in text.splitlines()],
                             ["xreal-control.zip", "xreal-linux-x86_64.AppImage"], "sorted by name")
            self.assertEqual(main.checksum_for("xreal-linux-x86_64.AppImage", text), release.sha256_file(a))
            self.assertEqual(main.checksum_for("xreal-control.zip", text), release.sha256_file(b))


class Bundle(unittest.TestCase):
    def _staging(self, d):
        os.makedirs(os.path.join(d, "dist"))
        for name in ("plugin.json", "main.py", "package.json", "LICENSE", "README.md"):
            with open(os.path.join(d, name), "w") as f:
                f.write(name)
        with open(os.path.join(d, "dist", "index.js"), "w") as f:
            f.write("bundle")
        # Development-only files that must not ship.
        os.makedirs(os.path.join(d, "src"))
        with open(os.path.join(d, "src", "index.tsx"), "w") as f:
            f.write("source")
        os.makedirs(os.path.join(d, "tests"))
        with open(os.path.join(d, "tests", "test_backend.py"), "w") as f:
            f.write("tests")
        with open(os.path.join(d, "dist", "index.js.map"), "w") as f:
            f.write("map")
        return d

    def test_bundle_holds_only_the_shippable_plugin_files(self):
        with tempfile.TemporaryDirectory() as d:
            self._staging(d)
            out = os.path.join(d, "xreal-control.zip")
            release.write_bundle(d, out)
            with zipfile.ZipFile(out) as z:
                names = sorted(z.namelist())
            self.assertEqual(names, sorted(release.PLUGIN_FILES))
            self.assertNotIn("index.js.map", names)
            self.assertFalse(any(n.startswith("src/") or n.startswith("tests/") for n in names))

    def test_bundle_is_byte_for_byte_reproducible(self):
        with tempfile.TemporaryDirectory() as d:
            self._staging(d)
            one = os.path.join(d, "one.zip")
            two = os.path.join(d, "two.zip")
            release.write_bundle(d, one)
            release.write_bundle(d, two)
            with open(one, "rb") as f1, open(two, "rb") as f2:
                self.assertEqual(f1.read(), f2.read())

    def test_building_without_a_built_plugin_fails(self):
        with tempfile.TemporaryDirectory() as d:
            with self.assertRaises(FileNotFoundError):
                release.bundle_files(d)


if __name__ == "__main__":
    unittest.main()
