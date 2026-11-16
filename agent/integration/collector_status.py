"""Translate upstream state without treating missing loss/lag data as zero."""
import math

from collector.native_status import BACKEND as NATIVE_BACKEND, NativeStatusError, project_native_status


def telemetry_status(provider):
    def snapshot():
        status = provider()
        if hasattr(status, "to_dict"):
            status = status.to_dict()
        if not isinstance(status, dict):
            return {"state": "unknown"}
        if status.get("backend") is not None:
            try:
                return project_native_status(status)
            except NativeStatusError:
                return {"state": "unknown"}
        state = status.get("state")
        if state == "external_prometheus":
            state = "disabled"  # No local child is operated in pull mode.
        elif state == "starting_or_status_unavailable":
            state = "starting"
        if state not in {"disabled", "starting", "running", "degraded", "stopped", "blocked"}:
            state = "unknown"
        result = {"state": state}
        support = status.get("strict_support")
        if (isinstance(support, dict) and support.get("reason_code") == "pinned_collector_buffer_contract_incompatible"
                and support.get("production_allowed") is False):
            result["remote_write_block_reason"] = "pinned_collector_buffer_contract_incompatible"
            result["remote_write_production_allowed"] = False
            if (support.get("receiver_response_bounded") is False
                    and support.get("receiver_response_reason_code") == "pinned_receiver_error_body_unbounded"):
                result["remote_write_receiver_response_bounded"] = False
                result["remote_write_receiver_response_block_reason"] = "pinned_receiver_error_body_unbounded"
            if (support.get("receiver_destination_pinned") is False
                    and type(support.get("receiver_destination_reason_code")) is str
                    and support["receiver_destination_reason_code"] == "remote_write_receiver_redirect_destination_not_pinned"):
                result["remote_write_receiver_destination_pinned"] = False
                result["remote_write_receiver_destination_block_reason"] = "remote_write_receiver_redirect_destination_not_pinned"
        scrape = status.get("last_scrape_success")
        result["last_scrape_success"] = scrape if type(scrape) is bool else None
        for source, destination in (("delivery_lag_seconds", "lag_seconds"),
                                    ("samples_dropped", "dropped_samples"),
                                    ("remote_write_samples_dropped", "remote_write_samples_dropped"),
                                    ("remote_write_packets_dropped", "remote_write_packets_dropped"),
                                    ("pending_bytes", "buffer_bytes")):
            value = status.get(source)
            if type(value) in (int, float) and math.isfinite(value) and 0 <= value < 2**53:
                result[destination] = value
        # queue_bytes_dropped is a byte counter, not a sample count. Missing
        # receiver timestamps cannot establish delivery lag or durable ingestion.
        return result
    return snapshot
