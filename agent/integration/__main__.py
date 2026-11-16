"""Run from the existing Agent directory: python -m integration --help."""
import argparse
import json
from pathlib import Path
import signal
import sys
import threading

AGENT_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AGENT_ROOT / "runtime"))
from cc_agent.manifest import Manifest
from cc_agent.service import AgentRuntime, create_server
from .identity_gate import load_gate


def main():
    parser = argparse.ArgumentParser(description="Read-only Agent with durable Ed25519 admission")
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--manifest", type=Path)
    source.add_argument("--profile", type=Path, help="unified public base/custom exporter profile")
    parser.add_argument("--identity-config", type=Path, required=True)
    parser.add_argument("--reader-config", type=Path,
                        help="explicit trusted Native Reader config; no enrollment or automatic grants")
    parser.add_argument("--tunnel-config", type=Path,
                        help="explicit Linux one-attempt TUN/Relay config; no enrollment or reconnect")
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--once", action="store_true", help="emit internal public heartbeat and exit")
    args = parser.parse_args()
    profile = None
    if args.profile:
        from .profile_manifest import ProfileManifest
        profile = ProfileManifest.load(args.profile)
        manifest = Manifest.from_dict(profile.runtime_manifest())
    else:
        manifest = Manifest.load(args.manifest)
    gate = reader_provider = reader_config = tunnel_config = tunnel_runner = None
    try:
        if args.tunnel_config is not None:
            from .tunnel_config import TunnelConfig
            tunnel_config = TunnelConfig.from_file(args.tunnel_config)
        if args.reader_config is not None:
            from .device_reader_config import ReaderGateConfig
            from .device_reader_gate import NativeDeviceReaderProvider
            reader_config = ReaderGateConfig.from_file(args.reader_config)
            reader_provider = NativeDeviceReaderProvider(reader_config)
        gate = load_gate(args.identity_config, manifest.agent_id, {
            "/v1/heartbeat": manifest.heartbeat_resource_id,
            "/v1/history": manifest.history_resource_id,
        })
        if reader_config is not None:
            from .sqlite_custody_gate import SqliteCustodyGate
            if type(gate) is not SqliteCustodyGate:
                raise ValueError("reader_requires_sqlite_custody_node_gate")
            reader_config.validate_node_config(gate.config)
        if tunnel_config is not None:
            from .sqlite_custody_gate import SqliteCustodyGate
            if type(gate) is not SqliteCustodyGate:
                raise ValueError("tunnel_requires_sqlite_custody_node_gate")
            tunnel_config.validate_node_config(gate.config, manifest.agent_id)
        if profile is not None:
            from .profile_runtime import ProfileRuntime
            runtime = ProfileRuntime(args.profile, profile, authorize=gate, state_dir=args.state_dir,
                                     reader_provider=reader_provider)
        else:
            runtime = AgentRuntime(manifest, authorize=gate, state_dir=args.state_dir,
                                   reader_provider=reader_provider)
        runtime.sample()
        if args.once:
            print(json.dumps(runtime.heartbeat(), separators=(",", ":")))
            return
        server = create_server(runtime)
        sampler = threading.Thread(target=runtime.sample_loop, daemon=True)

        def stop(signum, frame):
            runtime.stop_event.set()
            threading.Thread(target=server.shutdown, daemon=True).start()

        signal.signal(signal.SIGINT, stop)
        signal.signal(signal.SIGTERM, stop)
        try:
            if tunnel_config is not None:
                from .tunnel_config import TunnelRunner
                tunnel_runner = TunnelRunner(tunnel_config)
                tunnel_runner.start()
            sampler.start()
            # Binding remains numeric loopback even with a configured Reader.
            server.serve_forever(poll_interval=0.25)
        finally:
            runtime.stop_event.set()
            server.server_close()
            if sampler.ident is not None:
                sampler.join(timeout=2)
    finally:
        try:
            try:
                if tunnel_runner is not None:
                    tunnel_runner.close()
            finally:
                if reader_provider is not None:
                    reader_provider.close()
        finally:
            if hasattr(gate, "close"):
                gate.close()


if __name__ == "__main__":
    main()
