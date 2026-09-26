#!/usr/bin/env python3
"""Build/test/open the native macOS tuning companion, without an Xcode project."""
import argparse
import os
from pathlib import Path
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SCRATCH = ROOT / "target/swift-tuning"
BUNDLE = ROOT / "target/ARPG Tuning.app"


def swift(*args):
    return subprocess.run(["swift", *args, "--package-path", str(ROOT / "tools/tuning"),
                           "--scratch-path", str(SCRATCH)], check=True)


def build():
    swift("build", "-c", "release")
    binary = subprocess.check_output(["swift", "build", "-c", "release", "--show-bin-path",
                                      "--package-path", str(ROOT / "tools/tuning"),
                                      "--scratch-path", str(SCRATCH)], text=True).strip()
    contents = BUNDLE / "Contents"
    (contents / "MacOS").mkdir(parents=True, exist_ok=True)
    shutil.copy2(Path(binary) / "TuningCompanion", contents / "MacOS/TuningCompanion")
    with (contents / "Info.plist").open("wb") as handle:
        plistlib.dump({"CFBundleExecutable": "TuningCompanion", "CFBundleIdentifier": "dev.arpg.tuning",
                      "CFBundleName": "ARPG Tuning", "CFBundlePackageType": "APPL",
                      "CFBundleVersion": "1", "LSMinimumSystemVersion": "14.0",
                      "NSHighResolutionCapable": True}, handle)
    subprocess.run(["codesign", "--force", "--sign", "-", str(BUNDLE)], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["build", "test", "open"])
    parser.add_argument("--socket", default=os.environ.get("ARPG_TUNING_SOCKET", str(ROOT / "target/tuning.sock")))
    args = parser.parse_args()
    if args.command == "test":
        # Apple's Command Line Tools ship SwiftUI but not XCTest/Swift Testing.
        # Compile the real model and transport with a small async assertion runner.
        SCRATCH.mkdir(parents=True, exist_ok=True)
        sources = ROOT / "tools/tuning/Sources/TuningCompanion"
        checks = SCRATCH / "TuningChecks"
        subprocess.run(["swiftc", "-parse-as-library", "-swift-version", "6", "-warnings-as-errors",
                        str(sources / "Connection.swift"), str(sources / "Model.swift"),
                        str(ROOT / "tools/tuning/Tests/Checks.swift"),
                        "-o", str(checks)], check=True)
        subprocess.run([str(checks)], check=True)
        build()
    else:
        build()
        if args.command == "open":
            subprocess.run(["open", str(BUNDLE), "--args", "--socket", str(Path(args.socket).absolute())], check=True)
