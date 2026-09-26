"""Launcher readiness must identify its own game and preserve unrelated apps."""
import json
from concurrent.futures import Future
from pathlib import Path
import socket
import tempfile
import threading
import unittest
from unittest.mock import Mock, patch

import dev


class Launcher(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="arpg-dev-", dir="/tmp")
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "game.sock"
        self.game = Mock(pid=123, returncode=None)
        self.game.poll.return_value = None

    def peer(self, session):
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(self.path))
        server.listen(1)
        server.settimeout(2)
        completed = Future()
        def respond():
            try:
                with server, server.accept()[0] as client:
                    client.settimeout(2)
                    with client.makefile("rb") as request:
                        self.assertEqual(request.readline(1024), b'{"op":"state"}\n')
                    client.sendall(json.dumps({"ok": True, "state": {"session": session}}).encode() + b"\n")
                completed.set_result(None)
            except BaseException as error:
                completed.set_exception(error)
        thread = threading.Thread(target=respond)
        thread.start()
        self.addCleanup(thread.join)
        self.addCleanup(completed.result, 3)

    def test_readiness_requires_the_launched_process(self):
        self.peer(123)
        dev.wait_ready(self.game, self.path)

    def test_a_different_session_cannot_open_the_companion(self):
        self.peer(456)
        with self.assertRaisesRegex(RuntimeError, "different game process"):
            dev.wait_ready(self.game, self.path)

    def test_live_socket_is_refused_without_removal(self):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(self.path))
            server.listen(1)
            with self.assertRaisesRegex(RuntimeError, "already in use"):
                dev.refuse_active_socket(self.path)
            self.assertTrue(self.path.exists())

    def test_missing_socket_and_exited_game_fail_boundedly(self):
        with self.assertRaisesRegex(RuntimeError, "did not become ready"):
            dev.wait_ready(self.game, self.path, timeout=0.01)
        self.game.poll.return_value = 7
        self.game.returncode = 7
        with self.assertRaisesRegex(RuntimeError, "status 7"):
            dev.wait_ready(self.game, self.path)

    def test_failure_cleans_up_only_the_spawned_process(self):
        log = Path(self.temp.name) / "game.log"
        with patch.object(dev.subprocess, "Popen", return_value=self.game), \
             patch.object(dev, "wait_ready", side_effect=RuntimeError("not ready")), \
             patch.object(dev.companion, "open_app") as opened:
            with self.assertRaisesRegex(RuntimeError, "not ready"):
                dev.launch(Path("/game"), self.path, log)
        self.game.terminate.assert_called_once()
        self.game.wait.assert_called_once_with(timeout=3)
        opened.assert_not_called()

    def test_success_leaves_apps_independent_and_uses_the_same_socket(self):
        log = Path(self.temp.name) / "game.log"
        with patch.object(dev.subprocess, "Popen", return_value=self.game) as spawn, \
             patch.object(dev, "wait_ready") as ready, \
             patch.object(dev.companion, "open_app") as opened:
            self.assertEqual(dev.launch(Path("/game"), self.path, log), 123)
        self.assertEqual(spawn.call_args.kwargs["env"]["ARPG_COMPANION_SOCKET"], str(self.path))
        self.assertTrue(spawn.call_args.kwargs["start_new_session"])
        ready.assert_called_once_with(self.game, self.path)
        opened.assert_called_once_with(self.path)
        self.game.terminate.assert_not_called()
        self.game.wait.assert_not_called()


if __name__ == "__main__":
    unittest.main()
