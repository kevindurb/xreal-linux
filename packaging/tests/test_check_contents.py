import os, sys, tempfile, unittest
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import check_contents as cc

class Contents(unittest.TestCase):
    def test_a_clean_tree_passes_and_captures_fail(self):
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(f"{d}/usr/lib/xreal-linux/driver/xreal")
            open(f"{d}/usr/lib/xreal-linux/xreal-presenter", "w").close()
            self.assertEqual(cc.offenders(d), [])
            os.makedirs(f"{d}/captures")
            open(f"{d}/frame_0001_L_640x400.rgba", "w").close()
            open(f"{d}/config-0123456789abcdef.json", "w").close()
            names = [os.path.basename(p) for p in cc.offenders(d)]
            self.assertEqual(names, ["captures", "config-0123456789abcdef.json", "frame_0001_L_640x400.rgba"])

if __name__ == "__main__":
    unittest.main()
