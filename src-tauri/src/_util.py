"""Shared helpers for the tuning and shader workers.

Written next to worker.py in each session directory (the workers insert their own
directory onto sys.path, exactly as they already do for tuning_calibration.py), so
`from _util import ...` resolves both at runtime and under the unit tests. Scope is
deliberately limited to the byte-identical digest/file_digest/atomic helpers;
application_env / conditions_of / replay_argv are genuinely different per worker
and are NOT shared here.
"""

import hashlib
import os
import tempfile
from pathlib import Path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def atomic(path, data):
    path = Path(path)
    fd, tmp = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        if path.exists():
            os.chmod(tmp, path.stat().st_mode & 0o777)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)
