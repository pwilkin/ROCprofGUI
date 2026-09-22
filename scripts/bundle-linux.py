#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description="Build a local Debian package under .codex/linux-package.")
    parser.add_argument("--debug", action="store_true", help="Package a debug build instead of a release build.")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("Debian packaging requires a Linux build host")
    if shutil.which("dpkg-shlibdeps") is None:
        parser.error("Debian packaging requires dpkg-shlibdeps from dpkg-dev")
    root = Path(__file__).resolve().parents[1]
    output = root / ".codex/linux-package"
    work = output / "work"
    work.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "TMPDIR": str(work)}
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
        cwd=root, env=env, text=True,
    ))
    profile = "debug" if args.debug else "release"
    command = ["npm", "run", "tauri", "build", "--", "--no-bundle"]
    if args.debug:
        command.append("--debug")
    subprocess.run(command, cwd=root, env=env, check=True)
    source = Path(metadata["target_directory"]) / profile
    staging = output / "target" / profile
    staging.mkdir(parents=True, exist_ok=True)
    for name in ("rocprof-studio", "decoder-python.zip"):
        shutil.copy2(source / name, staging / name)
    abi = output / "abi"
    binary = abi / "debian/rocprof-studio/usr/bin/rocprof-studio"
    binary.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source / "rocprof-studio", binary)
    (abi / "debian/control").write_text(
        "Source: rocprof-studio\nSection: devel\nPriority: optional\nMaintainer: rocprof\n\n"
        "Package: rocprof-studio\nArchitecture: any\nDescription: Local ROCm profiling viewer\n"
    )
    scan = subprocess.run(["dpkg-shlibdeps", "-O", "-e", str(binary)], cwd=abi, env=env, text=True, capture_output=True)
    (output / "abi-dependencies.log").write_text(scan.stdout + scan.stderr)
    scan.check_returncode()
    dependency_lines = [line.removeprefix("shlibs:Depends=") for line in scan.stdout.splitlines() if line.startswith("shlibs:Depends=")]
    if len(dependency_lines) != 1 or not dependency_lines[0]:
        raise RuntimeError("dpkg-shlibdeps did not report runtime dependencies")
    detected_dependencies = [item.strip() for item in dependency_lines[0].split(",")]
    config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
    declared_dependencies = config["bundle"]["linux"]["deb"].get("depends", [])
    dependencies = list(dict.fromkeys([*declared_dependencies, *detected_dependencies]))
    bundle_config = {"bundle": {"active": True, "linux": {"deb": {"depends": dependencies}}}}
    bundle_env = {**env, "CARGO_TARGET_DIR": str(output / "target")}
    command = ["npm", "run", "tauri", "bundle", "--", "--bundles", "deb", "--config", json.dumps(bundle_config)]
    if args.debug:
        command.append("--debug")
    subprocess.run(command, cwd=root, env=bundle_env, check=True)
    packages = sorted((staging / "bundle/deb").glob("*.deb"), key=lambda p: p.stat().st_mtime_ns)
    if not packages:
        raise RuntimeError("Bundler produced no Debian package")
    package = packages[-1]
    receipt = {
        "package": str(package.relative_to(root)),
        "profile": profile,
        "detectedRuntimeDependencies": detected_dependencies,
        "sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
        "executableSha256BeforeBundlerPatch": hashlib.sha256((source / "rocprof-studio").read_bytes()).hexdigest(),
        "pythonPackageSha256": hashlib.sha256((source / "decoder-python.zip").read_bytes()).hexdigest(),
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
