import os, subprocess, sys, unittest
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import check_symbols as cs

class Symbols(unittest.TestCase):
    def test_ok_fixture_passes(self):
        self.assertEqual(cs.main(["x", "--objdump-text", f"{HERE}/objdump_ok.txt"]), 0)

    def test_too_new_fixture_fails_and_names_the_version(self):
        r = subprocess.run([sys.executable, f"{HERE}/../check_symbols.py", "--objdump-text", f"{HERE}/objdump_too_new.txt"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 1)
        self.assertIn("GLIBC_2.43", r.stdout)

    def test_versions_compare_numerically(self):
        self.assertEqual(cs.too_new("GLIBC_2.9 GLIBC_2.100"), [(2, 100)])

if __name__ == "__main__":
    unittest.main()
