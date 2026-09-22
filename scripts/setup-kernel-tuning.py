#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

PIN = 'c5b209828bfeb5f87d8624d4a553a7d9cd63a514'
REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description='Build the pinned AMD Kerncap capture and replay tools into the application data directory.')
    parser.add_argument('--rocm', default='/opt/rocm')
    parser.add_argument('--arch', required=True, help='Build host GPU, for example gfx1201')
    args = parser.parse_args()
    # Must match appdir::root() in src-tauri: the app derives the runtime path
    # from the parent of its kernel-tuning session root.
    home = os.environ.get('ROCPROFGUI_HOME') or str(Path.home() / '.rocprofgui')
    root = Path(home) / 'kernel-tuning'; root.mkdir(parents=True, exist_ok=True)
    source = root / 'intellikit'; build = root / 'build'; runtime = root / 'runtime'
    env = {**os.environ, 'TMPDIR': str(root)}
    def run(argv, cwd=REPO):
        subprocess.run([str(x) for x in argv], cwd=cwd, env=env, check=True)
    if not source.exists():
        source.mkdir()
        run(['git', 'init'], source)
        run(['git', 'remote', 'add', 'origin', 'https://github.com/AMDResearch/intellikit.git'], source)
        run(['git', 'fetch', '--depth', '1', 'origin', PIN], source)
        run(['git', 'checkout', '--detach', PIN], source)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=source, text=True).strip()
    if head != PIN: raise RuntimeError('Dependency checkout does not match pinned revision')
    patch = REPO / 'scripts/kerncap-replay.patch'
    applied = subprocess.run(['git', 'apply', '--reverse', '--check', str(patch)], cwd=source, capture_output=True)
    if applied.returncode:
        run(['git', 'apply', '--check', patch], source)
        run(['git', 'apply', patch], source)
    headers = root / 'rocm-headers'
    shutil.copytree(Path(args.rocm) / 'include/hsa', headers / 'hsa', dirs_exist_ok=True)
    run(['cmake', '-S', source / 'kerncap', '-B', build, '-DCMAKE_BUILD_TYPE=Release', f'-DROCM_PATH={args.rocm}', f'-DCMAKE_PREFIX_PATH={args.rocm}', f'-DCMAKE_HIP_ARCHITECTURES={args.arch}', f'-DCMAKE_HIP_FLAGS=-I{headers}', f'-DCMAKE_INSTALL_PREFIX={runtime}'])
    run(['cmake', '--build', build, '-j4']); run(['cmake', '--install', build])
    shutil.copy2(source / 'LICENSE', runtime / 'LICENSE.intellikit')
    (runtime / 'provenance.json').write_text(json.dumps({'repository': 'https://github.com/AMDResearch/intellikit', 'revision': PIN, 'patch': str(patch), 'architecture': args.arch, 'rocm': args.rocm}, indent=2))
    print(runtime)


if __name__ == '__main__':
    main()
