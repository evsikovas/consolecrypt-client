"""Package the pure Python files from the reviewed PyYAML release sdist."""

from base64 import urlsafe_b64encode
from hashlib import sha256
from io import BytesIO
from pathlib import Path
import sys
import tarfile
from zipfile import ZipFile, ZipInfo

SOURCE_HASH = "d76623373421df22fb4cf8817020cbb7ef15c725b9d5e45f17e189bfc384190f"
SOURCE_ROOT = "pyyaml-6.0.3/"
DIST_INFO = "pyyaml-6.0.3.dist-info/"
WHEEL_NAME = "pyyaml-6.0.3-py3-none-any.whl"


def build(source):
    if sha256(source).hexdigest() != SOURCE_HASH:
        raise ValueError("Expected the original PyYAML 6.0.3 source archive")
    files = {}
    with tarfile.open(fileobj=BytesIO(source), mode="r:gz") as archive:
        for member in archive:
            package_roots = (SOURCE_ROOT + "lib/yaml/", SOURCE_ROOT + "lib/_yaml/")
            if member.isfile() and member.name.startswith(package_roots) and member.name.endswith(".py"):
                files[member.name.removeprefix(SOURCE_ROOT + "lib/")] = archive.extractfile(member).read()
        headers, body = archive.extractfile(SOURCE_ROOT + "PKG-INFO").read().split(b"\n\n", 1)
        files[DIST_INFO + "METADATA"] = b"\n".join(
            line for line in headers.splitlines() if not line.startswith(b"Dynamic:")
        ) + b"\n\n" + body
        files[DIST_INFO + "licenses/LICENSE"] = archive.extractfile(SOURCE_ROOT + "LICENSE").read()
    files[DIST_INFO + "WHEEL"] = (
        b"Wheel-Version: 1.0\nGenerator: ConsoleCrypt\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
    )
    records = []
    output = BytesIO()
    with ZipFile(output, mode="w") as wheel:
        for name, content in sorted(files.items()):
            digest = urlsafe_b64encode(sha256(content).digest()).rstrip(b"=").decode("ascii")
            records.append(f"{name},sha256={digest},{len(content)}")
            wheel.writestr(ZipInfo(name), content)
        record_name = DIST_INFO + "RECORD"
        wheel.writestr(ZipInfo(record_name), "\n".join([*records, f"{record_name},,", ""]).encode("utf-8"))
    return output.getvalue()


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python package_pyyaml.py pyyaml-6.0.3.tar.gz")
    wheel = build(Path(sys.argv[1]).read_bytes())
    output = Path(__file__).with_name(WHEEL_NAME)
    output.write_bytes(wheel)
    print(f"{sha256(wheel).hexdigest()}  {output.name}")
