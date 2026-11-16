"""Fixed installed command only; never activated by main Agent config presence."""
import json
from pathlib import Path
import signal
import sys
import threading


def main():
    if sys.argv[1:] != ['outbound-control'] or sys.platform != 'linux':
        raise SystemExit(1)
    root = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(root / 'runtime'))
    # The installed launcher verified this exact signed package and own wheel
    # before entering here; no ambient package/PATH discovery or Python child.
    from cc_agent.outbound_control_daemon import run_daemon
    stop = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stop.set())
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    result = run_daemon(stop)
    print(json.dumps(result,sort_keys=True,separators=(',',':'),ensure_ascii=True),flush=True)
    raise SystemExit(0 if result['cleanupObserved'] and result['reason'] in {'stopped','expired'} else 1)


if __name__ == '__main__':
    main()
