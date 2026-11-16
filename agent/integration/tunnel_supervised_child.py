"""Fixed reviewed system-Python package child. No caller argv or interpreter search."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / 'python/site-packages/websockets-17.4-py3-none-any.whl'),
               str(ROOT / 'runtime'), str(ROOT)]


def main():
    if (sys.platform != 'linux' or (sys.version_info[:2], sys.executable) not in (
                ((3, 11), '/usr/bin/python3.11'), ((3, 12), '/usr/bin/python3.12')) or len(sys.argv) != 1
            or not sys.flags.isolated or not sys.flags.no_site or not sys.dont_write_bytecode):
        raise SystemExit(1)
    from cc_agent.tunnel_supervised import run_child
    try:
        run_child()
    except BaseException as error:
        # stdout belongs exclusively to the private bounded pipe protocol.
        error.__traceback__ = error.__context__ = error.__cause__ = None
        raise SystemExit(1) from None


if __name__ == '__main__':
    main()
