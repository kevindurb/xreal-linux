"""Tests for the viewer's sources and HTTP server. Run: python3 -m unittest tools/imu_web/test_server.py -v"""
import http.client
import json
import math
import struct
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import server  # noqa: E402
import sources  # noqa: E402

CAPTURE = Path(__file__).resolve().parents[2] / "captures" / "imu_yaw.bin"


def make_record(ts_ns, kind, vals, varying=b"\x38\x41"):
    rec = bytearray(sources.RECORD)
    rec[:len(sources.MAGIC)] = sources.MAGIC
    rec[6:8] = varying   # header bytes 6-7 differ between glasses sessions (38 41 vs 28 be)
    struct.pack_into("<Q", rec, sources.TS_OFFSET, ts_ns)
    struct.pack_into("<I", rec, sources.TYPE_OFFSET, kind)
    struct.pack_into("<6f", rec, sources.FLOAT_OFFSET, *vals)
    return bytes(rec)


class ParseTests(unittest.TestCase):
    def test_round_trip_and_resync(self):
        a = make_record(1_000_000, 0x0B, (0.1, 0.2, 0.3, 1.0, -9.7, 0.5))
        nan = make_record(2_000_000, 0x04, (math.nan,) * 6)
        b = make_record(3_000_000, 0x0B, (0.0, 0.0, 0.0, 0.0, -9.8, 0.0))
        stream = b"junk" + a + nan + b[:50]
        recs, rest = sources.parse_records(stream)
        self.assertEqual(len(recs), 1)                       # NaN-type record skipped, partial b held back
        self.assertAlmostEqual(recs[0][2], 0.2, places=5)
        recs2, _ = sources.parse_records(rest + b[50:])
        self.assertEqual(len(recs2), 1)
        self.assertEqual(recs2[0][0], 3_000_000)

    def test_header_bytes_6_7_are_not_matched(self):
        stream = b"".join(make_record(i * 1_000_000, 0x0B, (0, 0, 0, 0, -9.8, 0), varying=v)
                          for i, v in enumerate([b"\x38\x41", b"\x28\xbe", b"\x00\x00", b"\xff\xff"]))
        recs, _ = sources.parse_records(stream)
        self.assertEqual(len(recs), 4)

    @unittest.skipUnless(CAPTURE.exists(), "real capture not present")
    def test_real_capture(self):
        recs, _ = sources.parse_records(CAPTURE.read_bytes())
        self.assertGreater(len(recs), 15000)
        mags = [math.sqrt(r[4] ** 2 + r[5] ** 2 + r[6] ** 2) for r in recs]
        self.assertTrue(all(9.0 < m < 10.5 for m in mags[::50]), "accel magnitude should be about 1 g")
        span = (recs[-1][0] - recs[0][0]) / 1e9
        self.assertAlmostEqual(len(recs) / span, 1000, delta=30)


class SimTests(unittest.TestCase):
    def test_gravity_and_motion(self):
        import random
        rng = random.Random(0)
        s = [sources.sim_sample(i / 1000, rng) for i in range(0, 12000)]
        self.assertTrue(all(9.5 < math.sqrt(x[4] ** 2 + x[5] ** 2 + x[6] ** 2) < 10.1 for x in s))
        yaw = sum((x[2] - sources.SIM_BIAS[1]) / 1000 for x in s[4000:5500])   # first move: yaw left
        self.assertAlmostEqual(math.degrees(yaw), -45, delta=1.5)


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.httpd, self.hub, self.stop = server.build(sources.SimSource(), "127.0.0.1", 0, self.tmp.name)
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def tearDown(self):
        self.stop.set()
        self.hub.set_recording(False)
        self.httpd.shutdown()
        self.httpd.server_close()
        self.tmp.cleanup()

    def req(self, method, path, body=None):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        c.request(method, path, body=json.dumps(body) if body is not None else None)
        r = c.getresponse()
        data = r.read()
        c.close()
        return r.status, data

    def test_static_files(self):
        status, body = self.req("GET", "/")
        self.assertEqual(status, 200)
        self.assertIn(b"XREAL IMU check", body)
        for f in ("core.js", "app.js"):
            self.assertEqual(self.req("GET", "/" + f)[0], 200)
        self.assertEqual(self.req("GET", "/../server.py")[0], 404)
        self.assertEqual(self.req("GET", "/%2e%2e/server.py")[0], 404)
        self.assertEqual(self.req("GET", "/nope.js")[0], 404)

    def test_event_stream_delivers_samples(self):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        c.request("GET", "/events")
        r = c.getresponse()
        self.assertEqual(r.getheader("Content-Type"), "text/event-stream")
        got = 0
        for _ in range(200):
            line = r.fp.readline().decode()
            if line.startswith("data: ") and '"s"' in line:
                batch = json.loads(line[6:])["s"]
                self.assertEqual(len(batch[0]), 7)
                got += len(batch)
                if got > 100:
                    break
        c.close()
        self.assertGreater(got, 100)
        status, body = self.req("GET", "/api/status")
        self.assertTrue(json.loads(body)["connected"])

    def test_save_result_and_record(self):
        status, body = self.req("POST", "/api/result", {"directions": {"yaw-left": {"axis": "y", "sign": -1}}})
        self.assertEqual(status, 200)
        saved = Path(json.loads(body)["path"])
        self.assertTrue(saved.is_file() and saved.parent == Path(self.tmp.name))
        self.assertEqual(json.loads(saved.read_text())["directions"]["yaw-left"]["axis"], "y")

        status, body = self.req("POST", "/api/record", {"on": True})
        path = Path(json.loads(body)["recording"])
        threading.Event().wait(0.5)
        self.req("POST", "/api/record", {"on": False})
        lines = path.read_text().splitlines()
        self.assertGreater(len(lines), 50)
        self.assertEqual(len(json.loads(lines[0])), 7)

    def test_bad_requests(self):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        c.request("POST", "/api/result", body=b"not json")
        self.assertEqual(c.getresponse().status, 400)
        c.close()
        self.assertEqual(self.req("POST", "/api/other", {})[0], 404)


if __name__ == "__main__":
    unittest.main()
