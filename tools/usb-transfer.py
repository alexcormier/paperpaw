#!/usr/bin/env python3
"""Installs a plugin or adds books on a PaperPaw device over the USB cable,
with the cable transfer of every firmware (docs/usb-transfer.md): the
device asks on its screen first, and writes nothing before the reader
allows it. Also speaks to the desktop emulator (`paperpaw-emu --cable`).

  python tools/usb-transfer.py hello
  python tools/usb-transfer.py plugins
  python tools/usb-transfer.py install path/to/my-plugin
  python tools/usb-transfer.py books a.epub b.pdf
  python tools/usb-transfer.py --emulator "paperpaw-emu --cable" hello

--port names the serial port (found by itself on an X4 or X4 Pro).
Needs pyserial for a device: pip install pyserial.
"""
import argparse
import hashlib
import json
import shlex
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote

PREFIX = "PPT/1 "
DATA = 512
# Espressif's USB serial and JTAG unit, built into the X4's ESP32-C3 and
# the X4 Pro's ESP32-S3.
USB_ID = (0x303A, 0x1001)
TIMEOUT_S = 30
# How long the reader has to answer on the device.
ASK_S = 120


class Serial:
    def __init__(self, name):
        import serial
        from serial.tools import list_ports

        if name is None:
            found = [p.device for p in list_ports.comports() if (p.vid, p.pid) == USB_ID]
            if not found:
                sys.exit("No device found on USB; name its port with --port.")
            name = found[0]
        port = serial.Serial()
        port.port = name
        port.timeout = 0.2
        # Without DTR and RTS, opening the port does not reset the chip.
        port.dtr = False
        port.rts = False
        port.open()
        self.port = port
        self.buffer = b""

    def write(self, line):
        self.port.write(line.encode() + b"\n")

    def line(self, deadline):
        while b"\n" not in self.buffer:
            if time.monotonic() > deadline:
                return None
            self.buffer += self.port.read(4096)
        line, self.buffer = self.buffer.split(b"\n", 1)
        return line.decode("utf-8", "replace").strip()


class Emulator:
    def __init__(self, command):
        self.process = subprocess.Popen(
            shlex.split(command), stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True
        )

    def write(self, line):
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()

    def line(self, deadline):
        line = self.process.stdout.readline()
        return line.strip() if line else None


class Device:
    def __init__(self, link):
        self.link = link
        self.next = 1

    def request(self, method, target, body=b""):
        """Sends a request; its status and body."""
        n = self.next
        self.next += 1
        self.link.write(f"{PREFIX}{n} {method} {target} {len(body)}")
        for at in range(0, len(body), DATA):
            self.link.write(f"{PREFIX}{n} data {body[at:at + DATA].hex()}")
        status, data = None, b""
        deadline = time.monotonic() + TIMEOUT_S
        while True:
            line = self.link.line(deadline)
            if line is None:
                sys.exit(f"{method} {target}: no answer")
            # The log and other lines go by.
            if not line.startswith(f"{PREFIX}{n} "):
                continue
            word = line.split(" ", 3)[2]
            if word == "data":
                data += bytes.fromhex(line.split(" ", 3)[3])
            elif word == "end":
                return status, data.decode("utf-8", "replace")
            else:
                status = int(word)

    def json(self, method, target, body=b""):
        status, text = self.request(method, target, body)
        try:
            return status, json.loads(text)
        except ValueError:
            return status, {"text": text}

    def offer(self, lines):
        """Offers what is to come; True once the reader allowed it."""
        status, answer = self.json("POST", "/api/offer", "".join(lines).encode())
        if status != 202:
            sys.exit(f"offer refused ({status}): {answer}")
        print("Allow it on the device...")
        deadline = time.monotonic() + ASK_S
        while time.monotonic() < deadline:
            time.sleep(1)
            _, answer = self.json("GET", "/api/offer")
            if answer.get("state") == "allowed":
                return True
            if answer.get("state") == "refused":
                sys.exit("Not allowed on the device.")
        sys.exit("No answer on the device.")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def install(device, folder):
    folder = Path(folder)
    ini = (folder / "plugin.ini").read_text(encoding="utf-8")
    ids = [l.split("=", 1)[1].strip() for l in ini.splitlines() if l.strip().startswith("id")]
    plugin = ids[0] if ids else folder.name
    # Hidden files (.git, .DS_Store) are not the plugin's.
    names = sorted(
        p.relative_to(folder).as_posix()
        for p in folder.rglob("*")
        if p.is_file() and not any(part.startswith(".") for part in p.relative_to(folder).parts)
    )
    spaced = [n for n in names if " " in n]
    if spaced:
        sys.exit(f"names with spaces are not sent: {', '.join(spaced)}")
    files = [(name, (folder / name).read_bytes()) for name in names]
    device.offer(
        [f"plugin {plugin}\n"] + [f"{name} {len(data)} {sha256(data)}\n" for name, data in files]
    )
    for name, data in files:
        target = f"/api/plugin/{plugin}/{quote(name)}?sha256={sha256(data)}"
        status, answer = device.json("PUT", target, data)
        if status != 200:
            sys.exit(f"{name}: {status} {answer}")
        print(f"{name}: {len(data)} bytes, checked")
    status, answer = device.json("POST", f"/api/plugin/{plugin}")
    if status != 200:
        sys.exit(f"{plugin}: not installed ({status}): {answer}")
    restart = " (runs after a restart)" if answer.get("restart") else ""
    print(f"{plugin}: installed{restart}")


def books(device, paths):
    paths = [Path(p) for p in paths]
    hidden = [p.name for p in paths if p.name.startswith(".")]
    if hidden:
        sys.exit(f"hidden files are not books: {', '.join(hidden)}")
    files = [(path.name, path.read_bytes()) for path in paths]
    device.offer([f"book {name} {len(data)} {sha256(data)}\n" for name, data in files])
    for name, data in files:
        target = f"/api/book/{quote(name)}?sha256={sha256(data)}"
        status, answer = device.json("PUT", target, data)
        if status != 200:
            sys.exit(f"{name}: {status} {answer}")
        print(f"{name}: {answer.get('path')}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--port")
    parser.add_argument("--emulator", help="the emulator's command, e.g. 'paperpaw-emu --cable'")
    parser.add_argument("command", choices=["hello", "plugins", "install", "books"])
    parser.add_argument("paths", nargs="*")
    args = parser.parse_args()
    link = Emulator(args.emulator) if args.emulator else Serial(args.port)
    device = Device(link)
    if args.command == "hello":
        print(device.json("GET", "/api/hello")[1])
    elif args.command == "plugins":
        for plugin in device.json("GET", "/api/plugins")[1].get("plugins", []):
            print(f"{plugin['id']:<24} {plugin['version']:<12} {plugin['origin']:<9} "
                  f"{'on' if plugin['on'] else 'off'}")
    elif args.command == "install":
        if len(args.paths) != 1:
            sys.exit("install: one plugin folder")
        install(device, args.paths[0])
    else:
        if not args.paths:
            sys.exit("books: the files to add")
        books(device, args.paths)


if __name__ == "__main__":
    main()
