"""Paths supplied by the trusted launcher on macOS, fixed mounts on Linux."""
import os
from pathlib import Path
import sys


WORK = Path(os.environ.get('CC_WORK', '/work'))
SOURCE = Path(os.environ.get('CC_SOURCE', '/project'))
PROJECT = WORK / 'project'


def tool(name):
    return Path(os.environ.get('CC_TOOL', '/tool')) / name


def input_source(name):
    return sys.stdin.buffer if sys.platform == 'darwin' else tool(name).open('rb')
