#!/usr/bin/env python3
"""Capture real headless-browser screenshots of SubVault Web Demo served under /SubVault/.

Also asserts zero HTTP 404/5xx errors under subpath hosting.
"""

from __future__ import annotations

import functools
import http.server
import os
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WEB_DIR = ROOT / "web"
OUT_DIR = ROOT / "docs" / "screenshots"

BROWSER_CANDIDATES = [
    os.environ.get("SUBVAULT_BROWSER"),
    shutil.which("chromium"),
    shutil.which("chromium-browser"),
    shutil.which("google-chrome"),
    shutil.which("google-chrome-stable"),
    "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe",
    "/mnt/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
]


def find_browser() -> str:
    for candidate in BROWSER_CANDIDATES:
        if candidate and Path(candidate).exists():
            return candidate
    raise SystemExit("No headless Chromium/Chrome/Edge browser found.")


class SubpathCheckHandler(http.server.SimpleHTTPRequestHandler):
    statuses: list[tuple[str, int]] = []

    def send_response(self, code: int, message: str | None = None) -> None:
        self.statuses.append((self.path, code))
        super().send_response(code, message)

    def log_message(self, format: str, *args) -> None:  # noqa: A002
        pass


def to_browser_path(browser: str, path: Path) -> str:
    if browser.lower().endswith(".exe"):
        return subprocess.check_output(["wslpath", "-w", str(path)], text=True).strip()
    return str(path)


def capture(browser: str, url: str, dest: Path, width: int = 1440, height: int = 940) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    # Windows browsers may not write directly into WSL UNC paths cleanly on all setups;
    # write to a temp path if needed, then copy into repo.
    is_win = browser.lower().endswith(".exe")
    if is_win:
        win_tmp = Path("/mnt/c/Users/Public")
        if win_tmp.exists() and os.access(win_tmp, os.W_OK):
            tmp_shot = win_tmp / f"subvault-shot-{dest.name}"
        else:
            tmp_shot = dest
    else:
        tmp_shot = dest

    out_arg = to_browser_path(browser, tmp_shot)
    profile_dir = tempfile.mkdtemp(prefix="subvault-chrome-")
    try:
        cmd = [
            browser,
            "--headless=new",
            "--disable-gpu",
            "--no-sandbox",
            "--hide-scrollbars",
            "--force-device-scale-factor=2",
            f"--window-size={width},{height}",
            "--virtual-time-budget=3000",
            f"--screenshot={out_arg}",
            url,
        ]
        if not is_win:
            cmd.insert(-1, f"--user-data-dir={profile_dir}")
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=45)
        if proc.returncode != 0:
            raise SystemExit(f"Browser failed ({proc.returncode}):\n{proc.stderr}")
        if tmp_shot != dest:
            shutil.move(str(tmp_shot), str(dest))
        if not dest.exists() or dest.stat().st_size < 10_000:
            raise SystemExit(f"Screenshot missing or unexpectedly tiny: {dest}")
    finally:
        shutil.rmtree(profile_dir, ignore_errors=True)


def main() -> None:
    browser = find_browser()
    print(f"Using headless browser: {browser}")

    with tempfile.TemporaryDirectory(prefix="subvault-pages-stage-") as stage_root:
        stage_path = Path(stage_root)
        shutil.copytree(WEB_DIR, stage_path / "SubVault")

        SubpathCheckHandler.statuses = []
        handler = functools.partial(SubpathCheckHandler, directory=str(stage_path))
        httpd = socketserver.TCPServer(("127.0.0.1", 0), handler)
        port = httpd.server_address[1]
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()

        try:
            shots = [
                (
                    f"http://127.0.0.1:{port}/SubVault/?view=ledger&select=v-2026-09-receipt.pdf&demoEdit=1",
                    OUT_DIR / "ledger-overview.png",
                    1440,
                    1120,
                ),
                (
                    f"http://127.0.0.1:{port}/SubVault/?view=audit",
                    OUT_DIR / "dedup-and-audit.png",
                    1440,
                    990,
                ),
            ]
            for url, dest, w, h in shots:
                print(f"Capturing {url} -> {dest.relative_to(ROOT)}")
                capture(browser, url, dest, width=w, height=h)
        finally:
            httpd.shutdown()
            httpd.server_close()

        bad = [(p, c) for p, c in SubpathCheckHandler.statuses if c >= 400 and not p.endswith("/favicon.ico")]
        print(f"Served {len(SubpathCheckHandler.statuses)} HTTP requests under /SubVault/ subpath.")
        if bad:
            raise SystemExit(f"Subpath static asset errors detected: {bad}")
        print("Verified: 0 broken static assets under /SubVault/ subpath.")


if __name__ == "__main__":
    main()
