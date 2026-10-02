#!/usr/bin/env python3
"""Prepare only tracked, public client/shared sources for an isolated builder."""
import argparse
import importlib.util
from pathlib import Path
import shutil
import subprocess


def stage(root: Path, output: Path) -> int:
    if output.exists():
        raise ValueError('Choose a new staging directory')
    spec = importlib.util.spec_from_file_location('public_export', root / 'client/scripts/export-source.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    names = subprocess.check_output(['git', '-C', str(root), 'ls-files', '-z']).decode().split('\0')
    selected = sorted({name for name in names if name and module.publishable(name)
                       and (name.startswith(('client/', 'crates/')) or name in {'LICENSE', 'rust-toolchain.toml'})})
    for name in selected:
        source = root / name
        if source.is_symlink() or not source.is_file():
            raise ValueError(f'Expected a regular tracked source: {name}')
    output.mkdir(parents=True)
    for name in selected:
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / name, destination)
    return len(selected)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    count = stage(arguments.root.resolve(), arguments.output.resolve())
    print(f'Staged {count} public build source files; no private history or login files')
