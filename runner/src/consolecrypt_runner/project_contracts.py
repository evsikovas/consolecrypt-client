"""Reserved root filenames have identical semantics on Windows and Unix."""
import os
from pathlib import Path

from .errors import require


def contract_path(project, filename):
    with os.scandir(project) as entries:
        for entry in entries:
            if entry.name.isascii() and entry.name.lower() == filename:
                require(entry.name == filename, 'project_contract_name',
                        'Use the exact lowercase project contract filename: ' + filename)
    return Path(project) / filename
