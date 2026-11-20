"""ConsoleCrypt execution helper. Importing never installs or executes a job."""

import platform
import sys

PROTOCOL_VERSION = 1
VERSION = "0.1.0"


def expected_ansible_core_version():
    if sys.platform == "darwin" and platform.machine() == "arm64" and sys.version_info[:2] == (3, 15):
        return "2.22.0"
    return "2.20.11"
