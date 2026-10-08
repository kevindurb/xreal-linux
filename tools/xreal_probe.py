#!/usr/bin/env python3
"""Send ONE read-only request to the XREAL glasses and log what comes back.

This is the first (and, by design, a very small) host-to-glasses message of this project. Safety limits:
  * exactly one packet is sent per run, and never retried;
  * only the read-only "Get" requests on READ_ONLY below are allowed, all with an empty request body;
  * nothing that writes, configures, starts, stops, reboots or updates can be sent from this tool;
  * the connection is closed after a short timeout.

Packet (see docs/xreal-link-messages.md): msg_id u16 BE, payload length u32 BE, payload = protobuf Base{ field 3 = request },
and an empty request is `1a 00`. Example, NRGlassesGetSWVersion (10013 = 0x271d): 27 1d 00 00 00 02 1a 00

With --txid the control-port framing documented by Skarian/one-xr (MIT, XrControlSession.kt) is used instead: the payload starts with a
u32 BE transaction id with the top bit set, followed by the request body (`18 00` for a getter); the response comes back with the same
magic and id. Only port 52999 and the read-only getters in READ_ONLY_TX are allowed in that mode.

usage: xreal_probe.py --port 52990 [--id 10013] [--host 169.254.2.1] [--listen 1.5] [--wait 3] [--out FILE]
       xreal_probe.py --txid --port 52999 --id 0x271f [--json-out FILE]
"""
import argparse
import json
import os
import socket
import struct
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from xreal_link import decode_pb, parse_packets  # noqa: E402

# Allowlist: id -> name. Read-only getters whose request body is empty (docs/xreal-link-messages.md, section 8).
READ_ONLY = {
    10013: "NRGlassesGetSWVersion",
    10265: "NRGlassesGetStartupState",
    10027: "NRGlassesGetSystemVersion",
    10028: "NRGlassesGetHWVersion",
    10029: "NRGlassesGetDspVersion",
}
EMPTY_BODY = bytes.fromhex("1a00")  # Base{ field 3 = {} }

# --txid mode: magic -> name. Read-only getters of the control port (52999), request body `18 00`.
READ_ONLY_TX = {
    0x271D: "GetSoftwareVersion",
    0x271F: "GetConfig",
    0x2729: "GetId",
    0x272D: "GetDspVersion",
}
TX_BODY = bytes.fromhex("1800")
TX_ID = 1


def build_tx(magic):
    payload = struct.pack(">I", TX_ID | 0x80000000) + TX_BODY
    return struct.pack(">HI", magic, len(payload)) + payload


def decode_tx_response(packets, magic):
    """Find the response to our request among parsed packets; return (transaction_id, string_or_None, body_bytes)."""
    for _off, mid, pl in packets:
        if mid != magic or len(pl) < 4:
            continue
        txid = struct.unpack(">I", pl[:4])[0] & 0x7FFFFFFF
        body = pl[4:]
        # response body: field 4 (0x22), length varint, nested message; a string/JSON value is nested field 2 (0x12)
        i = 0
        def varint(buf, i):
            v = sh = 0
            while True:
                b = buf[i]; i += 1
                v |= (b & 0x7F) << sh; sh += 7
                if not b & 0x80:
                    return v, i
        try:
            if body and body[0] == 0x22:
                n, i = varint(body, 1)
                nested = body[i:i + n]
                if nested and nested[0] == 0x12:
                    n2, j = varint(nested, 1)
                    return txid, nested[j:j + n2].decode("utf-8", "replace"), body
        except IndexError:
            pass
        return txid, None, body
    return None


def build(msg_id):
    return struct.pack(">HI", msg_id, len(EMPTY_BODY)) + EMPTY_BODY


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="169.254.2.1")
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--id", type=lambda v: int(v, 0), default=10013, help="message id, decimal or 0x hex")
    ap.add_argument("--listen", type=float, default=1.5, help="seconds to listen before sending")
    ap.add_argument("--wait", type=float, default=3.0, help="seconds to wait for a reply after sending")
    ap.add_argument("--out", help="write everything received (before and after) to this file")
    ap.add_argument("--dry-run", action="store_true", help="build and print the packet, do not connect")
    ap.add_argument("--txid", action="store_true", help="control-port framing with a transaction id (port 52999 only)")
    ap.add_argument("--json-out", help="with --txid: write the string/JSON value of the response to this file")
    a = ap.parse_args()

    if a.txid:
        if a.id not in READ_ONLY_TX:
            sys.exit("refusing: id 0x%x is not on the read-only --txid allowlist %s" % (a.id, [hex(k) for k in sorted(READ_ONLY_TX)]))
        if a.port != 52999:
            sys.exit("refusing: --txid is only for the control port 52999")
        names, pkt = READ_ONLY_TX, build_tx(a.id)
    else:
        if a.id not in READ_ONLY:
            sys.exit("refusing: id %d is not on the read-only allowlist %s" % (a.id, sorted(READ_ONLY)))
        if not 52990 <= a.port <= 52999:
            sys.exit("refusing: port %d is outside 52990-52999" % a.port)
        names, pkt = READ_ONLY, build(a.id)
    print("request: %s (id %d) -> %s:%d : %s" % (names[a.id], a.id, a.host, a.port, pkt.hex(" ")), flush=True)
    if a.dry_run:
        return

    result = {"host": a.host, "port": a.port, "id": a.id, "name": names[a.id], "sent": pkt.hex(),
              "before": b"", "after": b"", "events": []}
    t0 = time.monotonic()
    s = socket.socket()
    s.settimeout(4)
    try:
        s.connect((a.host, a.port))
    except OSError as e:
        print("connect failed:", e)
        result["connect_error"] = str(e)
        s.close()
        finish(a, result)
        return
    print("connected in %.2f s" % (time.monotonic() - t0), flush=True)

    def read_for(seconds):
        buf = b""
        end = time.monotonic() + seconds
        s.settimeout(0.3)
        while time.monotonic() < end:
            try:
                d = s.recv(65536)
            except socket.timeout:
                continue
            except OSError as e:
                result["events"].append("recv error: %s" % e)
                break
            if not d:
                result["events"].append("closed by glasses at %.2f s" % (time.monotonic() - t0))
                break
            buf += d
            if a.txid and buf:
                pk, _ = parse_packets(buf)
                if wait_for_reply[0] and any(mid == a.id for _o, mid, _p in pk):
                    break
        return buf

    wait_for_reply = [False]
    result["before"] = read_for(a.listen)
    print("received %d bytes before sending" % len(result["before"]), flush=True)
    try:
        s.sendall(pkt)  # the one and only packet
        result["events"].append("sent at %.2f s" % (time.monotonic() - t0))
    except OSError as e:
        result["events"].append("send error: %s" % e)
    else:
        wait_for_reply[0] = True
        result["after"] = read_for(a.wait)
    s.close()
    finish(a, result)


def finish(a, result):
    before, after = result["before"], result["after"]
    print("received %d bytes after sending" % len(after))
    for e in result["events"]:
        print("event:", e)
    for label, data in (("before", before), ("after", after)):
        if not data:
            continue
        print("--- %s: %s" % (label, data[:96].hex(" ")))
        pk, skipped = parse_packets(data)
        for off, mid, pl in pk:
            line = "    packet id %d len %d" % (mid, len(pl))
            try:
                line += " protobuf " + json.dumps(decode_pb(pl))
            except Exception:
                line += " raw " + pl[:48].hex(" ")
            print(line)
        if skipped:
            print("    (%d bytes did not parse as packets)" % skipped)
    if a.txid and after:
        pk, _ = parse_packets(after)
        got = decode_tx_response(pk, a.id)
        if got is None:
            print("no response with magic 0x%x among the packets received" % a.id)
        else:
            txid, text, body = got
            print("response: transaction id %d, body %d bytes, first bytes %s" % (txid, len(body), body[:16].hex(" ")))
            if text is not None:
                print("string value: %d chars" % len(text))
                try:
                    obj = json.loads(text)
                    print("valid JSON; top-level keys: %s" % (sorted(obj) if isinstance(obj, dict) else type(obj).__name__))
                except ValueError:
                    print("not JSON; first 80 chars: %r" % text[:80])
                if a.json_out:
                    with open(a.json_out, "w") as f:
                        f.write(text)
                    print("wrote", a.json_out)
    if a.out:
        with open(a.out, "wb") as f:
            f.write(before + after)
        meta = {k: (v.hex() if isinstance(v, bytes) else v) for k, v in result.items() if k not in ("before", "after")}
        meta["before_bytes"], meta["after_bytes"] = len(before), len(after)
        json.dump(meta, open(a.out + ".json", "w"), indent=1)


if __name__ == "__main__":
    main()
