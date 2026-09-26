#!/usr/bin/env python3
"""Build and launch the game and native companion as independent applications."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import companion

ROOT = Path(__file__).resolve().parents[1]


def refuse_active_socket(path):
    # The game owns socket creation and stale-socket recovery. Never unlink here.
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as peer:
        peer.settimeout(0.5)
        try:
            peer.connect(str(path))
        except (FileNotFoundError, ConnectionRefusedError):
            return
    raise RuntimeError(f"{path} is already in use. Close that game or choose --socket.")


def wait_ready(game, path, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if game.poll() is not None:
            raise RuntimeError(f"Game exited during startup (status {game.returncode}).")
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as peer:
                peer.settimeout(min(1.0, max(0.001, deadline - time.monotonic())))
                peer.connect(str(path))
                peer.sendall(b'{"op":"state"}\n')
                with peer.makefile("rb") as response:
                    line = response.readline(1024 * 1024 + 1)
                if len(line) > 1024 * 1024 or not line.endswith(b"\n"):
                    raise RuntimeError("Invalid readiness reply from the tooling socket.")
                reply = json.loads(line)
                if not isinstance(reply, dict) or not isinstance(reply.get("state"), dict):
                    raise RuntimeError("Invalid readiness reply from the tooling socket.")
                if reply["state"].get("session") != game.pid:
                    raise RuntimeError("The tooling socket belongs to a different game process.")
                if reply.get("ok") is True and game.poll() is None:
                    return
        except (FileNotFoundError, ConnectionRefusedError, socket.timeout):
            pass
        time.sleep(min(0.1, max(0, deadline - time.monotonic())))
    raise RuntimeError(f"Game did not become ready within {timeout} seconds.")


def launch(game_path, socket_path, log_path):
    refuse_active_socket(socket_path)
    socket_path.parent.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, ARPG_COMPANION_SOCKET=str(socket_path))
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w") as log:
        game = subprocess.Popen([str(game_path)], cwd=ROOT, env=environment,
                                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                start_new_session=True)
    try:
        wait_ready(game, socket_path)
        companion.open_app(socket_path)
    except BaseException:
        # Startup is the launcher's responsibility; established sessions are not.
        if game.poll() is None:
            game.terminate()
            try:
                game.wait(timeout=3)
            except subprocess.TimeoutExpired:
                game.kill()
                game.wait()
        raise
    return game.pid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true", help="launch existing binaries")
    parser.add_argument("--socket", type=Path, default=Path(os.environ.get("ARPG_COMPANION_SOCKET", ROOT / "target/companion.sock")))
    parser.add_argument("--game", type=Path, default=ROOT / "target/release/arpg", help="game executable (also supports packaged builds)")
    args = parser.parse_args()
    socket_path, game_path = args.socket.absolute(), args.game.absolute()
    log_path = ROOT / "target/dev-game.log"
    try:
        refuse_active_socket(socket_path)
        if not args.no_build:
            cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
            subprocess.run([cargo, "build", "--release", "-p", "arpg"], cwd=ROOT, check=True)
            companion.build()
        pid = launch(game_path, socket_path, log_path)
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"dev: {error}\nGame startup log: {log_path}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("dev: startup cancelled", file=sys.stderr)
        return 130
    print(f"Game started (PID {pid}); companion opened.\nSocket: {socket_path}\nGame log: {log_path}\nClose either app independently.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
