#!/usr/bin/env python3
"""Web viewer for the XREAL IMU stream. Standard library only (the Deck has no numpy).

    python3 server.py                                   # live from the glasses on the Deck
    python3 server.py --source replay:../../captures/imu_yaw.bin
    python3 server.py --source sim                      # synthetic data, for developing the UI

Binds to 127.0.0.1 by default. To view it from another machine use an SSH tunnel
(ssh -L 8765:localhost:8765 steamdeck). --bind 0.0.0.0 exposes an unauthenticated API that can
write files into the captures directory, so only do that on a network you trust.
"""
import argparse
import json
import queue
import threading
import time
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import sources

HERE = Path(__file__).resolve().parent
STATIC = HERE / "static"
DEFAULT_CAPTURES = HERE.parents[1] / "captures"
MAX_BODY = 1_000_000
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
         ".css": "text/css; charset=utf-8"}


class Hub:
    """Fans samples out to browser connections and keeps source status."""

    def __init__(self, source, captures_dir):
        self.source, self.captures_dir = source, Path(captures_dir)
        self.subs, self.lock = set(), threading.Lock()
        self.info = {"kind": source.kind, "label": source.label, "connected": False, "error": None,
                     "samples": 0, "rate": 0.0, "recording": None}
        self._rate_t, self._rate_n = time.monotonic(), 0
        self._rec = None

    def set_connected(self, ok, error=None):
        self.info["connected"], self.info["error"] = ok, None if ok else error

    def subscribe(self):
        q = queue.Queue(maxsize=400)
        with self.lock:
            self.subs.add(q)
        return q

    def unsubscribe(self, q):
        with self.lock:
            self.subs.discard(q)

    def _fan_out(self, msg):
        with self.lock:
            for q in list(self.subs):
                try:
                    q.put_nowait(msg)
                except queue.Full:
                    pass  # slow browser: drop rather than stall the source

    def publish_reset(self):
        self._fan_out(json.dumps({"reset": True}))

    def publish(self, samples):
        self._fan_out(json.dumps({"s": samples}, separators=(",", ":")))
        self.info["samples"] += len(samples)
        self._rate_n += len(samples)
        now = time.monotonic()
        if now - self._rate_t >= 1.0:
            self.info["rate"] = round(self._rate_n / (now - self._rate_t), 1)
            self._rate_t, self._rate_n = now, 0
        if self._rec:
            for s in samples:
                self._rec.write(json.dumps(s, separators=(",", ":")) + "\n")

    def set_recording(self, on):
        if on and not self._rec:
            self.captures_dir.mkdir(parents=True, exist_ok=True)
            path = self.captures_dir / f"imu-session-{datetime.now():%Y%m%d-%H%M%S}.jsonl"
            self._rec = open(path, "w", buffering=1)
            self.info["recording"] = str(path)
        elif not on and self._rec:
            self._rec.close()
            self._rec, self.info["recording"] = None, None
        return self.info["recording"]


def make_handler(hub):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def _send(self, code, body, ctype="application/json"):
            if isinstance(body, (dict, list)):
                body = json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def _body_json(self):
            n = int(self.headers.get("Content-Length", 0))
            if n > MAX_BODY:
                raise ValueError("body too large")
            return json.loads(self.rfile.read(n) or b"{}")

        def do_GET(self):
            path = self.path.split("?")[0]
            if path == "/events":
                return self._events()
            if path == "/api/status":
                return self._send(200, hub.info)
            name = "index.html" if path == "/" else path.lstrip("/")
            f = (STATIC / name.removeprefix("static/")).resolve()
            if f.parent != STATIC or not f.is_file() or f.suffix not in TYPES:
                return self._send(404, {"error": "not found"})
            self._send(200, f.read_bytes(), TYPES[f.suffix])

        def do_POST(self):
            try:
                data = self._body_json()
                if self.path == "/api/result":
                    hub.captures_dir.mkdir(parents=True, exist_ok=True)
                    out = hub.captures_dir / f"axis-map-{datetime.now():%Y%m%d-%H%M%S}.json"
                    out.write_text(json.dumps(data, indent=2))
                    return self._send(200, {"path": str(out)})
                if self.path == "/api/record":
                    return self._send(200, {"recording": hub.set_recording(bool(data.get("on")))})
            except (ValueError, OSError) as e:
                return self._send(400, {"error": str(e)})
            self._send(404, {"error": "not found"})

        def _events(self):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "keep-alive")
            self.end_headers()
            q = hub.subscribe()
            try:
                self.wfile.write(f"event: hello\ndata: {json.dumps(hub.info)}\n\n".encode())
                self.wfile.flush()
                while True:
                    try:
                        msg = q.get(timeout=15)
                        self.wfile.write(f"data: {msg}\n\n".encode())
                    except queue.Empty:
                        self.wfile.write(b": keepalive\n\n")
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError, OSError):
                pass
            finally:
                hub.unsubscribe(q)

    return Handler


def build(source, bind="127.0.0.1", port=8765, captures_dir=DEFAULT_CAPTURES):
    """Returns (httpd, hub, stop_event). The source thread is started but not the HTTP loop."""
    hub = Hub(source, captures_dir)
    stop = threading.Event()
    threading.Thread(target=source.run, args=(hub, stop), daemon=True).start()
    httpd = ThreadingHTTPServer((bind, port), make_handler(hub))
    httpd.daemon_threads = True
    return httpd, hub, stop


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--source", default="tcp://169.254.1.1:52998",
                    help="tcp://HOST[:PORT] | replay:FILE | sim  (default: %(default)s)")
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--captures-dir", default=str(DEFAULT_CAPTURES))
    args = ap.parse_args()
    httpd, hub, stop = build(sources.make_source(args.source), args.bind, args.port, args.captures_dir)
    print(f"{hub.info['label']}\nserving on http://{args.bind}:{args.port}/  (Ctrl-C to stop)")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
        hub.set_recording(False)


if __name__ == "__main__":
    main()
