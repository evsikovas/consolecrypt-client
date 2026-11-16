import argparse
import json
import signal
import threading

from .manifest import Manifest
from .service import AgentRuntime, create_server


def main():
    parser = argparse.ArgumentParser(description="Isolated loopback monitoring agent (HTTP deny by default)")
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--state-dir")
    parser.add_argument("--once", action="store_true", help="emit internal public heartbeat and exit")
    args = parser.parse_args()
    manifest = Manifest.load(args.manifest)
    runtime = AgentRuntime(manifest, state_dir=args.state_dir)
    runtime.sample()
    if args.once:
        print(json.dumps(runtime.heartbeat(), separators=(",", ":")))
        return
    server = create_server(runtime)
    sampler = threading.Thread(target=runtime.sample_loop, daemon=True)
    sampler.start()
    def stop(signum, frame):
        runtime.stop_event.set()
        threading.Thread(target=server.shutdown, daemon=True).start()
    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        server.serve_forever(poll_interval=0.25)
    finally:
        runtime.stop_event.set()
        server.server_close()
        sampler.join(timeout=1)


if __name__ == "__main__":
    main()
