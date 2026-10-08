#!/usr/bin/env python3
"""Tests for tools/xreal_link.py: synthetic packets always, real captures when captures/ exists (it is git-ignored)."""
import os
import struct
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import xreal_link as xl  # noqa: E402

CAPS = os.path.join(HERE, "..", "captures")


def packet(mid, payload):
    return struct.pack(">HI", mid, len(payload)) + payload


class Framing(unittest.TestCase):
    def test_parses_back_to_back_packets(self):
        data = packet(10033, b"\0" * 32) + packet(10294, b"\1" * 128) + packet(10122, b"\x1a\x00")
        pk, skipped = xl.parse_packets(data)
        self.assertEqual([m for _, m, _ in pk], [10033, 10294, 10122])
        self.assertEqual(skipped, 0)
        self.assertEqual([len(p) for _, _, p in pk], [32, 128, 2])

    def test_resyncs_after_garbage(self):
        data = b"\xff\xff\xff" + packet(10033, b"\0" * 32) + b"\x00" * 5 + packet(10033, b"\2" * 32)
        pk, skipped = xl.parse_packets(data)
        self.assertEqual(len(pk), 2)
        self.assertGreaterEqual(skipped, 3)

    def test_truncated_final_packet_is_dropped(self):
        data = packet(10033, b"\0" * 32) + packet(10033, b"\0" * 32)[:20]
        pk, _ = xl.parse_packets(data)
        self.assertEqual(len(pk), 1)

    def test_ignores_out_of_range_ids(self):
        pk, skipped = xl.parse_packets(struct.pack(">HI", 5, 4) + b"abcd")
        self.assertEqual(pk, [])
        self.assertGreater(skipped, 0)


class Protobuf(unittest.TestCase):
    def test_temperature_event(self):
        got = xl.decode_pb(bytes.fromhex("1a07080115 9a992f42".replace(" ", "")))
        self.assertEqual(got[0][0], 3)
        self.assertEqual(got[0][1], "msg")
        inner = {f: v for f, _, v in got[0][2]}
        self.assertEqual(inner[1], 1)
        self.assertAlmostEqual(inner[2]["float"], 43.9, places=2)

    def test_empty_request_body(self):
        self.assertEqual(xl.decode_pb(bytes.fromhex("1a00")), [(3, "bytes", "")])

    def test_rejects_field_zero(self):
        with self.assertRaises(ValueError):
            xl.decode_pb(b"\x00")


class Names(unittest.TestCase):
    def test_known_ids(self):
        self.assertEqual(xl.name_of(10294), "IMU record (stream)")
        self.assertEqual(xl.name_of(10013), "NRGlassesGetSWVersion")
        self.assertEqual(xl.name_of(1), "?")


class RecorderRoundTrip(unittest.TestCase):
    """capture_eye.py against a fake glasses on localhost, then read the capture back with xreal_link."""

    def test_record_and_read_back(self):
        import json
        import socket
        import subprocess
        import tempfile
        import threading
        import time

        ports = {52996: (10033, b"\0" * 32, 100), 52999: (10122, bytes.fromhex("1a05150000 7042".replace(" ", "")), 20)}

        def serve(port, mid, payload, rate):
            srv = socket.socket()
            srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                srv.bind(("127.0.0.1", port))
            except OSError:
                return None
            srv.listen(1)

            def run():
                srv.settimeout(5)
                try:
                    c, _ = srv.accept()
                except OSError:
                    return
                end = time.time() + 2.5
                try:
                    while time.time() < end:
                        c.sendall(packet(mid, payload))
                        time.sleep(1.0 / rate)
                except OSError:
                    pass
                finally:
                    c.close()
                    srv.close()

            threading.Thread(target=run, daemon=True).start()
            return srv

        servers = [serve(p, *v) for p, v in ports.items()]
        if not all(servers):
            self.skipTest("test ports in use")
        out = os.path.join(tempfile.mkdtemp(), "cap")
        subprocess.run([sys.executable, os.path.join(HERE, "capture_eye.py"), out, "--host", "127.0.0.1",
                        "--ports", "52996,52999,52997", "--phases", "a:0.5,b:1.0"], check=True, capture_output=True)
        meta = json.load(open(os.path.join(out, "meta.json")))
        self.assertEqual([p["name"] for p in meta["phases"]], ["a", "b"])
        self.assertGreater(meta["ports"]["port52996"]["msg_ids"]["10033"]["count"], 50)
        self.assertEqual(meta["ports"]["port52996"]["skipped_bytes"], 0)
        self.assertGreater(meta["ports"]["port52999"]["msg_ids"]["10122"]["count"], 10)
        self.assertEqual(meta["ports"]["port52997"]["bytes"], 0)  # nothing listening: recorded as an error, not a crash
        self.assertIsNotNone(meta["ports"]["port52997"]["error"])
        inputs = dict(xl.load_inputs(out))
        pk, skipped = xl.parse_packets(inputs["port52996.raw"])
        self.assertEqual(skipped, 0)
        self.assertTrue(all(m == 10033 for _, m, _ in pk))


@unittest.skipUnless(os.path.exists(os.path.join(CAPS, "imu_yaw.bin")), "no local captures")
class RealCaptures(unittest.TestCase):
    def test_imu_capture_is_clean(self):
        data = open(os.path.join(CAPS, "imu_yaw.bin"), "rb").read()
        pk, skipped = xl.parse_packets(data)
        self.assertEqual(skipped, 0)
        self.assertTrue(all(m == 10294 and len(p) == 128 for _, m, p in pk))
        by = {}
        for off, _, _ in pk:
            by.setdefault(struct.unpack_from("<I", data, off + 30)[0], []).append(struct.unpack_from("<Q", data, off + 14)[0])
        self.assertEqual(sorted(by), [4, 11])  # magnetometer and gyro/accel records are interleaved
        for kind, ts in by.items():
            self.assertTrue(all(b > a for a, b in zip(ts, ts[1:])), kind)
        rate = lambda ts: (len(ts) - 1) / ((ts[-1] - ts[0]) / 1e9)
        self.assertAlmostEqual(rate(by[11]), 1000.0, delta=5)
        self.assertAlmostEqual(rate(by[4]), 400.0, delta=3)

    def test_magnetometer_magnitude_at_rest(self):
        f = os.path.join(CAPS, "xreal_52998_still.bin")
        if not os.path.exists(f):
            self.skipTest("no still capture")
        data = open(f, "rb").read()
        pk, _ = xl.parse_packets(data)
        mags = []
        for off, _, _ in pk:
            if struct.unpack_from("<I", data, off + 30)[0] == 4:
                v = struct.unpack_from("<3f", data, off + 58)
                mags.append(sum(x * x for x in v) ** 0.5)
        mean = sum(mags) / len(mags)
        self.assertTrue(30 < mean < 70, mean)  # Earth's field, microtesla

    def test_camera_frames(self):
        f = os.path.join(CAPS, "cam_52997_sample.bin")
        if not os.path.exists(f):
            self.skipTest("no camera sample")
        data = open(f, "rb").read()
        pk, _ = xl.parse_packets(data)
        frames = [(o, m, p) for o, m, p in pk if m == 10056]
        self.assertGreaterEqual(len(frames), 5)
        self.assertTrue(all(len(p) == 193856 for _, _, p in frames))
        ts = [struct.unpack_from("<Q", data, o + 23)[0] for o, _, _ in frames]
        steps = [(b - a) / 1e6 for a, b in zip(ts, ts[1:])]
        self.assertTrue(all(15.0 < s < 18.5 for s in steps), steps)  # ~60 fps

    def test_event_capture(self):
        f = os.path.join(CAPS, "mv_52999.bin")
        if not os.path.exists(f):
            self.skipTest("no event capture")
        pk, skipped = xl.parse_packets(open(f, "rb").read())
        self.assertEqual(skipped, 0)
        self.assertTrue(all(m == 10122 for _, m, _ in pk))
        for _, _, p in pk:
            self.assertEqual(xl.decode_pb(p)[0][0], 3)


class ControlSession(unittest.TestCase):
    """tools/xreal_session.py: the frame builder, the allowlist and the response decoder (no hardware)."""

    def setUp(self):
        import xreal_session as xs
        self.xs = xs

    def test_get_config_frame_is_the_one_sent_to_the_glasses(self):
        self.assertEqual(self.xs.build_request(10015, self.xs.DEFAULT_BODY, 1).hex(" "), "27 1f 00 00 00 06 80 00 00 01 18 00")

    def test_transaction_id_has_the_top_bit_and_length_counts_it(self):
        pkt = self.xs.build_request(10273, bytes.fromhex("1800"), 0x1234)
        self.assertEqual(struct.unpack(">HI", pkt[:6]), (10273, 6))
        self.assertEqual(struct.unpack(">I", pkt[6:10])[0], 0x80001234)

    def test_getters_are_always_allowed(self):
        for mid in (10013, 10015, 10085, 10273, 10003, 10005, 10008, 10044):
            self.assertTrue(self.xs.check_allowed(mid, False))

    def test_camera_requests_need_the_flag(self):
        for mid in (10047, 10053, 10054):
            with self.assertRaises(ValueError):
                self.xs.check_allowed(mid, False)
            self.assertTrue(self.xs.check_allowed(mid, True))

    def test_everything_else_is_refused_even_with_the_flag(self):
        # set brightness, set input mode, set space mode, reboot, shutdown, set SDK version, a Release-like id
        for mid in (10012, 10274, 10284, 10034, 10035, 10014, 10055, 0, 65535):
            with self.assertRaises(ValueError):
                self.xs.check_allowed(mid, True)

    def test_decodes_a_numeric_and_an_empty_response(self):
        self.assertEqual(self.xs.decode_response(bytes.fromhex("22040800 1001".replace(" ", ""))), [(1, "varint", 0), (2, "varint", 1)])
        self.assertEqual(self.xs.decode_response(bytes.fromhex("2200")), [])

    def test_unparseable_response_is_kept_as_hex(self):
        self.assertEqual(self.xs.decode_response(b"\xff\xff")["raw"], "ffff")


if __name__ == "__main__":
    unittest.main()
