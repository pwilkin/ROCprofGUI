#!/usr/bin/env python3
import argparse
import ast
import hashlib
import json
from pathlib import Path
import zipfile


def version(path, name):
    for node in ast.parse(path.read_text()).body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == name for target in node.targets
        ):
            return ast.literal_eval(node.value)
    raise ValueError(f"Missing {name} in {path}")


def package(decoder, dependencies, output):
    decoder_python = decoder / "python" / "rocprof_trace_decoder"
    elftools = dependencies / "elftools"
    versions = {
        "rocprof-trace-decoder": version(decoder_python / "_version.py", "__version__"),
        "pyelftools": version(elftools / "__init__.py", "__version__"),
    }
    if versions != {"rocprof-trace-decoder": "0.2.2", "pyelftools": "0.33"}:
        raise ValueError(f"Unqualified Python dependency versions: {versions}")
    files = {}
    for source in (decoder_python, elftools):
        for path in sorted(source.rglob("*.py")):
            if path.is_symlink():
                raise ValueError(f"Dependency source symlink is not supported: {path}")
            files[f"{source.name}/{path.relative_to(source).as_posix()}"] = path.read_bytes()
    files["licenses/rocprof-trace-decoder.txt"] = (decoder / "LICENSE").read_bytes()
    files["licenses/pyelftools.txt"] = (
        dependencies / "pyelftools-0.33.dist-info" / "licenses" / "LICENSE"
    ).read_bytes()
    manifest = {
        "schema": 1,
        "versions": versions,
        "files": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())},
    }
    files["decoder-package.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".pending")
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, data in sorted(files.items()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o644 << 16
                archive.writestr(info, data)
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)
    print(json.dumps({"path": str(output), "sha256": hashlib.sha256(output.read_bytes()).hexdigest(), "files": len(files), "versions": versions}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Package qualified pure-Python ATT dependencies for Tauri.")
    parser.add_argument("--decoder-source", type=Path, required=True)
    parser.add_argument("--dependencies", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("third_party/decoder-python/decoder-python.zip"))
    args = parser.parse_args()
    package(args.decoder_source, args.dependencies, args.output)
