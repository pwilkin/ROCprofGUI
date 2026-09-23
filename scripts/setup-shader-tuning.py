#!/usr/bin/env python3
"""Builds the Vulkan shader-tuning runtime (capture layer and replay harness) from
tools/vkcap into the application data directory, where the app looks for it."""
import argparse
import glob
import os
from pathlib import Path
import re
import shutil
import subprocess

REPO = Path(__file__).resolve().parents[1]


def header_version(include):
    core = Path(include) / 'vulkan/vulkan_core.h'
    m = re.search(r'#define VK_HEADER_VERSION (\d+)', core.read_text()) if core.is_file() else None
    return int(m[1]) if m else -1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--include', help='Vulkan header directory (default: the newest of $VULKAN_SDK, ~/vulkan/*/x86_64 and /usr)')
    parser.add_argument('--cxx', default=os.environ.get('CXX', 'g++'))
    args = parser.parse_args()
    candidates = [args.include] if args.include else [
        os.path.join(os.environ['VULKAN_SDK'], 'include') if os.environ.get('VULKAN_SDK') else None,
        *glob.glob(os.path.expanduser('~/vulkan/*/x86_64/include')),
        '/usr/include',
    ]
    candidates = [c for c in candidates if c and header_version(c) >= 0]
    if not candidates:
        raise SystemExit('No Vulkan headers found; install libvulkan-dev or set --include')
    # Newer headers let the layer record more of the application's device features.
    include = max(candidates, key=header_version)
    # Must match appdir::root() in src-tauri.
    home = Path(os.environ.get('ROCPROFGUI_HOME') or Path.home() / '.rocprofgui')
    runtime = home / 'shader-tuning/runtime'
    for sub in ('lib', 'bin'):
        (runtime / sub).mkdir(parents=True, exist_ok=True)
    flags = ['-std=c++17', '-O2', '-Wall', '-Wextra', '-Wno-unused-parameter', '-Wno-missing-field-initializers', f'-I{include}']
    source = REPO / 'tools/vkcap'
    layer = runtime / 'lib/libVkLayer_rocprof_capture.so'
    subprocess.run([args.cxx, *flags, '-fPIC', '-shared', '-fvisibility=hidden', '-o', str(layer), str(source / 'vkcap_layer.cpp')], check=True)
    loader = shutil.which('pkg-config') and subprocess.run(['pkg-config', '--libs', 'vulkan'], capture_output=True, text=True).stdout.split()
    subprocess.run([args.cxx, *flags, '-o', str(runtime / 'bin/vkreplay'), str(source / 'vkreplay.cpp'), *(loader or ['-lvulkan'])], check=True)
    shutil.copy(source / 'VkLayer_rocprof_capture.json', runtime / 'lib/VkLayer_rocprof_capture.json')
    print(f'Built with Vulkan headers {header_version(include)} from {include}')
    print(f'Runtime: {runtime}')


if __name__ == '__main__':
    main()
