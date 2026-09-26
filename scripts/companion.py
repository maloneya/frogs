#!/usr/bin/env python3
"""Build/test/open the native macOS tooling companion, without an Xcode project."""
import argparse
import hashlib
import os
from pathlib import Path
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
# Swift module caches embed source paths. Verification worktrees can share a
# target directory, but must not reuse another checkout's native module cache.
SCRATCH = ROOT / "target/swift-companion" / hashlib.sha256(str(ROOT).encode()).hexdigest()[:12]
BUNDLE = ROOT / "target/ARPG Companion.app"


def swift(*args):
    return subprocess.run(["swift", *args, "--package-path", str(ROOT / "tools/companion"),
                           "--scratch-path", str(SCRATCH)], check=True)


def build():
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
    subprocess.run([cargo, "build", "--release", "-p", "arpg-authoring", "--bin", "arpg-author"], cwd=ROOT, check=True)
    swift("build", "-c", "release")
    binary = subprocess.check_output(["swift", "build", "-c", "release", "--show-bin-path",
                                      "--package-path", str(ROOT / "tools/companion"),
                                      "--scratch-path", str(SCRATCH)], text=True).strip()
    contents = BUNDLE / "Contents"
    (contents / "MacOS").mkdir(parents=True, exist_ok=True)
    shutil.copy2(Path(binary) / "ARPGCompanion", contents / "MacOS/ARPGCompanion")
    shutil.copy2(ROOT / "target/release/arpg-author", contents / "MacOS/arpg-author")
    with (contents / "Info.plist").open("wb") as handle:
        plistlib.dump({"CFBundleExecutable": "ARPGCompanion", "CFBundleIdentifier": "dev.arpg.companion",
                      "CFBundleName": "ARPG Companion", "CFBundlePackageType": "APPL",
                      "CFBundleVersion": "1", "LSMinimumSystemVersion": "14.0",
                      "NSHighResolutionCapable": True}, handle)
    subprocess.run(["codesign", "--force", "--sign", "-", str(BUNDLE)], check=True)


def open_app(socket_path):
    # A new instance honors --socket even when another companion is running.
    subprocess.run(["open", "-n", str(BUNDLE), "--args", "--socket", str(Path(socket_path).absolute()), "--scenes", str(ROOT / "scenes")], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["build", "test", "open"])
    parser.add_argument("--socket", default=os.environ.get("ARPG_COMPANION_SOCKET", str(ROOT / "target/companion.sock")))
    args = parser.parse_args()
    if args.command == "test":
        # Apple's Command Line Tools ship SwiftUI but not XCTest/Swift Testing.
        # Compile the real model and transport with a small async assertion runner.
        build()
        SCRATCH.mkdir(parents=True, exist_ok=True)
        sources = ROOT / "tools/companion/Sources/ARPGCompanion"
        checks = SCRATCH / "CompanionChecks"
        subprocess.run(["swiftc", "-parse-as-library", "-swift-version", "6", "-warnings-as-errors",
                        str(sources / "Connection.swift"), str(sources / "Model.swift"),
                        str(sources / "SceneDocument.swift"), str(sources / "Authoring.swift"),
                        str(ROOT / "tools/companion/Tests/Checks.swift"),
                        "-o", str(checks)], check=True)
        subprocess.run([str(checks)], check=True, env=dict(os.environ, ARPG_AUTHORING_BIN=str(ROOT / "target/release/arpg-author")))
    else:
        build()
        if args.command == "open":
            open_app(args.socket)
