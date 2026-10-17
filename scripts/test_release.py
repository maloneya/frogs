"""Dry-run packaging keeps runtime files and launches from the extracted folder."""
from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Release(unittest.TestCase):
    @contextmanager
    def fixture(self):
        with tempfile.TemporaryDirectory(prefix="arpg-release-") as temporary:
            directory = Path(temporary)
            checkout, commands = [directory / name for name in ("checkout", "commands")]
            (checkout / "scripts").mkdir(parents=True)
            commands.mkdir()
            for name in ("release.sh", "dev.py", "companion.py"):
                shutil.copy2(ROOT / "scripts" / name, checkout / "scripts" / name)
            # Own the scene setup; packaged layout must not make tests depend on playable scenes.
            (checkout / "scenes").mkdir()
            (checkout / "scenes" / "test.ron").write_text('(scene: (engine: (name: "Package check")))')
            for name in ("world", "characters/wolf", "weapons/thornbound-staff/equipped/sword-glaive", "audio"):
                shutil.copytree(ROOT / "assets" / name, checkout / "assets" / name)
            (checkout / ".gitignore").write_text("target/\ndist/\n")
            for arguments in (("init", "-q"), ("add", "."), ("-c", "user.name=Release test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture")):
                subprocess.run(["git", *arguments], cwd=checkout, check=True, capture_output=True)
            self.executable(commands / "cargo", '''#!/bin/sh
if [ "$1" = pkgid ]; then echo 'arpg@0.1.0'; exit; fi
mkdir -p target/release
cat > target/release/arpg <<'GAME'
#!/bin/sh
for asset in assets/world/ground.glb assets/world/chest/open.glb assets/world/woodland/pine.glb assets/weapons/thornbound-staff/equipped/sword-glaive/player.glb assets/characters/wolf/wolf.glb assets/characters/wolf/wolf.shadow.glb assets/audio/cleave.wav; do
    test -s "$asset" || exit 1
done
pwd
GAME
chmod +x target/release/arpg
''')
            # Builds and code signing are outside this packaging check. Never launch a GUI.
            self.executable(commands / "python3", '''#!/bin/sh
mkdir -p 'target/ARPG Companion.app/Contents/MacOS'
printf 'test bundle' > 'target/ARPG Companion.app/Contents/MacOS/ARPGCompanion'
''')
            for name in ("strip", "codesign"):
                self.executable(commands / name, "#!/bin/sh\nexit 0\n")
            environment = dict(os.environ, PATH=f"{commands}:{os.environ['PATH']}")
            yield directory, checkout, commands, environment

    def test_relocated_payload_has_runtime_assets_and_independent_launchers(self):
        with self.fixture() as (directory, checkout, commands, environment):
            # Untracked source is part of the working-tree build, even when no tracked file changed.
            (checkout / "untracked-source.txt").write_text("uncommitted input")
            refused = subprocess.run(["bash", "scripts/release.sh", "--skip-gate"], cwd=checkout,
                                     env=environment, capture_output=True, text=True)
            self.assertNotEqual(refused.returncode, 0)
            self.assertIn("source has uncommitted or untracked files", refused.stderr)
            packaged = subprocess.run(["bash", "scripts/release.sh", "--dry-run", "--skip-gate"], cwd=checkout,
                                      env=environment, check=True, capture_output=True, text=True)
            self.assertIn("includes uncommitted working-tree content", packaged.stderr)
            archive = next((checkout / "dist").glob("*.tar.gz"))
            extracted = directory / "extracted"
            extracted.mkdir()
            with tarfile.open(archive) as package:
                package.extractall(extracted)
            payload = next(extracted.iterdir())
            # Source checkout no longer exists. Finder can supply any initial directory.
            shutil.rmtree(checkout)
            launched = subprocess.run([str(payload / "play.command")], cwd=directory, check=True,
                                      capture_output=True, text=True)
            self.assertEqual(launched.stdout.strip(), str(payload))
            self.assertTrue((payload / "target/ARPG Companion.app/Contents/MacOS/ARPGCompanion").is_file())
            self.assertTrue((payload / "scripts/dev.py").is_file())
            self.assertTrue((payload / "scripts/companion.py").is_file())
            self.assertTrue((payload / "assets/audio/licenses/kenney-rpg.txt").is_file())
            self.assertIn("uncommitted working-tree content", (payload / "README.txt").read_text())
            self.assertFalse(list((payload / "assets").rglob("*.blend")))
            self.assertFalse(list((payload / "assets").rglob("*.py")))
            self.assertIn('cd "$(dirname "$0")"', (payload / "tools.command").read_text())

    def test_publication_refuses_source_or_head_changes_during_build_and_keeps_the_archive(self):
        for drift in ("tracked", "untracked", "head"):
            with self.subTest(drift=drift), self.fixture() as (_, checkout, commands, environment):
                # Local refs satisfy release admission without contacting a remote.
                subprocess.run(["git", "remote", "add", "origin", "https://example.invalid/arpg.git"], cwd=checkout, check=True)
                subprocess.run(["git", "update-ref", "refs/remotes/origin/main", "HEAD"], cwd=checkout, check=True)
                published = checkout / "target/published"
                self.executable(commands / "gh", '''#!/bin/sh
if [ "$1" = release ] && [ "$2" = view ]; then exit 1; fi
if [ "$1" = release ] && [ "$2" = create ]; then touch target/published; fi
''')
                # Mutate source after the initial clean check, while the build runs.
                build = commands / "python3"
                build.write_text(build.read_text() + '''
case "$ARPG_PACKAGE_TEST_DRIFT" in
    tracked) printf '\n# edited during build\n' >> scripts/dev.py ;;
    untracked) printf 'new input' > new-source.txt ;;
    head) git -c user.name='Release test' -c user.email=test@example.invalid commit --allow-empty -qm 'new HEAD during build' ;;
esac
''')
                result = subprocess.run(["bash", "scripts/release.sh", "--skip-gate"], cwd=checkout,
                                        env=dict(environment, ARPG_PACKAGE_TEST_DRIFT=drift), capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                reason = "HEAD changed" if drift == "head" else "source changed"
                self.assertIn(f"{reason} during release preparation", result.stderr)
                archives = list((checkout / "dist").glob("*.tar.gz"))
                self.assertEqual(len(archives), 1, result.stdout + result.stderr)
                self.assertIn(f"archive preserved at {archives[0].relative_to(checkout)}", result.stderr)
                with tarfile.open(archives[0]) as package:
                    self.assertTrue(any(member.name.endswith("/arpg") for member in package.getmembers()))
                self.assertFalse(published.exists(), "refused source must never reach gh release create")

    @staticmethod
    def executable(path, contents):
        path.write_text(contents)
        path.chmod(0o755)


if __name__ == "__main__":
    unittest.main()
