# PyYAML for native macOS Runner on Python 3.15

PyYAML 6.0.3 is distributed under MIT. Its PyPI release contains wheels through CPython 3.14. The macOS ARM64 Python 3.15 lock uses an embedded wheel containing the upstream pure Python modules and license.

- [Official PyYAML 6.0.3 source release](https://pypi.org/project/PyYAML/6.0.3/#files) · SHA-256 `d76623373421df22fb4cf8817020cbb7ef15c725b9d5e45f17e189bfc384190f`
- Wheel SHA-256 `b377f6c8f576b6dd8e2ae2452845f82bd860c301bef2cc83200f622ea91038e6`

Reproduce the wheel with `python runner/vendor/package_pyyaml.py pyyaml-6.0.3.tar.gz`. The script verifies the source archive, copies the Python modules and original license, and writes deterministic wheel metadata.
