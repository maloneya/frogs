"""Render and package a character review with the same Python used for this command."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__, epilog=(
        'All other options go to preview.py, including --after/--before, --source, --clips, '
        '--views, --modes, --samples poses, --frames and --resolution. '
        'Run Blender preview.py -- --help for their complete definitions.'))
    parser.add_argument('output', type=Path, help='new review directory')
    parser.add_argument('--blender', default='blender', help='Blender executable')
    parser.add_argument('--references', nargs='+', type=Path, default=[], help='reference images for the critic')
    args, render_options = parser.parse_known_args()
    output = args.output.resolve()
    log_path = None
    try:
        if output.exists():
            raise FileExistsError(f'Choose a new review directory: {output}')
        # Fail before rendering, instead of discovering a missing image runtime afterwards.
        try:
            from preview_gifs import package, reference_inputs
        except ModuleNotFoundError as error:
            if error.name == 'PIL':
                raise RuntimeError('Pillow is missing. Run review.py with the Python returned by '
                                   'Codex load_workspace_dependencies, or another Python with Pillow.') from error
            raise
        references = reference_inputs(args.references)
        blender = shutil.which(args.blender)
        if blender is None:
            raise FileNotFoundError(f'Blender executable not found: {args.blender}')
        with tempfile.NamedTemporaryFile(prefix='arpg-character-review-', suffix='.log', delete=False) as log:
            log_path = Path(log.name)
            result = subprocess.run([blender, '--background', '--factory-startup', '--disable-autoexec',
                                     '--python-exit-code', '1', '--python', str(HERE / 'preview.py'),
                                     '--', str(output), *render_options], stdout=log, stderr=subprocess.STDOUT)
        if result.returncode == 0:
            destination = output / 'render.log'
            log_path.rename(destination)
            log_path = destination
        if result.returncode:
            raise RuntimeError(f'Blender exited with status {result.returncode}; packaging was not started')
        package(output, references)
    except (OSError, RuntimeError, ValueError, KeyError) as error:
        print(f'review: {error}', file=sys.stderr)
        if log_path:
            print(f'Render log: {log_path}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
