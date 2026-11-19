"""Trusted bootstrap inside the namespace, before any project code imports."""
import os
import resource
import shutil
import sys
from .worker_paths import PROJECT, SOURCE, WORK


def main():
    if sys.argv[1:2] != ["--"] or len(sys.argv) < 3:
        raise SystemExit(64)
    os.umask(0o077)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_NOFILE, (512, 512))
    file_limit=(128 if sys.argv[2:5]==[sys.executable,'-m','consolecrypt_runner.git_worker'] else 64)*1024*1024
    resource.setrlimit(resource.RLIMIT_FSIZE, (file_limit, file_limit))
    if sys.platform == 'linux':
        resource.setrlimit(resource.RLIMIT_AS, (1536 * 1024 * 1024, 1536 * 1024 * 1024))
        resource.setrlimit(resource.RLIMIT_NPROC, (256, 256))
    resource.setrlimit(resource.RLIMIT_CPU, (3600, 3615))
    (WORK / 'home').mkdir(mode=0o700)
    shutil.copytree(SOURCE, PROJECT)
    os.chdir(PROJECT)
    os.environ["PYTHONSAFEPATH"] = "1"
    os.execv(sys.argv[2], sys.argv[2:])


if __name__ == "__main__":
    main()
