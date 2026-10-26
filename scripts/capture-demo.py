"""Record Aetherquill's screen on a reMarkable 2 for a demo video.

Reads the app's own window buffer, /dev/shm/qtfb_<key> (1404x1872 RGB565, shared
by AppLoad), over SSH. Read-only on the tablet: it runs `ls` and `cat`, writes
nothing there and never touches xochitl. On this computer: Python 3 with
Pillow + numpy, ssh, and ffmpeg for the video.

  python scripts/capture-demo.py --seconds 40           # record while you perform the shot list
  python scripts/capture-demo.py --seconds 40 --linkedin  # also a 1080x1350 (4:5) cut
  python scripts/capture-demo.py --selftest             # no tablet needed

Start Aetherquill from AppLoad first. The frame rate depends on the USB link
(each frame is 5.3 MB raw); the script prints what it measured. Frames are
replayed at their real timing, so the video runs at true speed.
"""
import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image

W, H = 1404, 1872
FRAME = W * H * 2  # RGB565
PAPER = (242, 239, 232)

SHOT_LIST = """Shot list (about 40 s; go slowly, e-ink rewards it):
  1. Draw a big ring, then a fire sigil inside it. Wait for the cast.
  2. Tap SPELLS, then NEXT, then PREV.
  3. Pick Light Beam. Let it ink and cast.
  4. Tap UNDO (the whole spell goes), then REDO.
  5. Tap SPELLS, pick Converging Wind, let it cast. Stop."""


def to_rgb(raw: bytes) -> Image.Image:
    """RGB565 little-endian -> 8-bit RGB."""
    v = np.frombuffer(raw, dtype="<u2").reshape(H, W)
    r = ((v >> 11) & 0x1F) * 255 // 31
    g = ((v >> 5) & 0x3F) * 255 // 63
    b = (v & 0x1F) * 255 // 31
    return Image.fromarray(np.dstack([r, g, b]).astype(np.uint8), "RGB")


def find_buffer(host: str) -> str:
    out = subprocess.run(["ssh", host, "ls -t /dev/shm/qtfb_* 2>/dev/null"], capture_output=True, text=True).stdout.split()
    if not out:
        sys.exit("no /dev/shm/qtfb_* on the tablet: start Aetherquill from AppLoad first")
    # ponytail: newest buffer = the app just launched; pass --buffer if AppLoad has several apps open
    return out[0]


def record(host: str, buffer: str, seconds: float, frames_dir: Path, every: float) -> list[tuple[float, Path]]:
    # One SSH session streams back-to-back frames; the tablet only runs cat + sleep.
    remote = f"while :; do cat {buffer}; sleep {every}; done"
    proc = subprocess.Popen(["ssh", host, remote], stdout=subprocess.PIPE)
    shots, t0, last = [], None, None
    try:
        while True:
            raw = proc.stdout.read(FRAME)
            if len(raw) < FRAME:
                break
            now = time.monotonic()
            t0 = t0 if t0 is not None else now
            if now - t0 > seconds:
                break
            if raw == last:  # unchanged screen: keep the earlier frame, longer
                continue
            last = raw
            path = frames_dir / f"{len(shots):05d}.png"
            to_rgb(raw).save(path)
            shots.append((now - t0, path))
            print(f"\r{now - t0:5.1f}s  {len(shots)} distinct frames", end="", flush=True)
    finally:
        proc.kill()
    print()
    return shots


def encode(shots: list[tuple[float, Path]], seconds: float, out: Path, scale: str) -> None:
    """Variable-timing frames -> constant 30 fps mp4 via ffmpeg's concat durations."""
    listing = out.with_suffix(".txt")
    with listing.open("w") as f:
        for (t, path), nxt in zip(shots, [s[0] for s in shots[1:]] + [seconds]):
            f.write(f"file '{path.resolve().as_posix()}'\nduration {max(nxt - t, 1 / 30):.3f}\n")
        f.write(f"file '{shots[-1][1].resolve().as_posix()}'\n")
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(listing),
                    "-t", f"{seconds:.3f}", "-vf", f"{scale},fps=30,format=yuv420p", "-c:v", "libx264", "-crf", "18", "-movflags", "+faststart", str(out)], check=True)
    listing.unlink()
    print(f"wrote {out}")


def selftest() -> None:
    v = np.zeros((H, W), dtype="<u2")
    v[0, 0], v[0, 1], v[0, 2] = 0xFFFF, 0xF800, 0x8410  # white, red, the app's mid-gray
    img = to_rgb(v.tobytes())
    assert img.size == (W, H)
    assert img.getpixel((0, 0)) == (255, 255, 255)
    assert img.getpixel((1, 0)) == (255, 0, 0)
    assert img.getpixel((2, 0)) == (131, 129, 131)  # 16/31, 32/63, 16/31
    assert img.getpixel((3, 0)) == (0, 0, 0)
    print("selftest ok")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="root@10.11.99.1")
    ap.add_argument("--buffer", help="/dev/shm/qtfb_<key> (default: the newest one)")
    ap.add_argument("--seconds", type=float, default=40)
    ap.add_argument("--every", type=float, default=0.0, help="pause between frames on the tablet, seconds")
    ap.add_argument("--out", type=Path, default=Path("demo-capture"))
    ap.add_argument("--linkedin", action="store_true", help="also write a 1080x1350 (4:5) cut")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if not shutil.which("ffmpeg"):
        sys.exit("ffmpeg not found on PATH")
    frames = a.out / "frames"
    frames.mkdir(parents=True, exist_ok=True)
    buffer = a.buffer or find_buffer(a.host)
    print(f"recording {buffer} for {a.seconds:.0f}s\n\n{SHOT_LIST}\n")
    shots = record(a.host, buffer, a.seconds, frames, a.every)
    if not shots:
        sys.exit("no frames received")
    shutil.copy(shots[-1][1], a.out / "last-frame.png")  # a settled still for the post's thumbnail
    print(f"{len(shots)} distinct frames in {a.seconds:.0f}s")
    encode(shots, a.seconds, a.out / "aetherquill-demo.mp4", "scale=1080:-2:flags=lanczos")
    if a.linkedin:
        pad = f"scale=1012:1350:flags=lanczos,pad=1080:1350:34:0:color=0x{''.join(f'{c:02x}' for c in PAPER)}"
        encode(shots, a.seconds, a.out / "aetherquill-demo-linkedin.mp4", pad)


if __name__ == "__main__":
    main()
