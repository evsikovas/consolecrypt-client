"""Pinned collector adapter; no TSDB, WAL or remote-write implementation."""
from .manager import (CollectorError, CollectorPlan, CollectorSupervisor,
                      CollectorStatus, validate_manifest, render_pull_config,
                      state_from_metrics, exporter_privileges, StrictSupport,
                      strict_support)

__all__ = ["CollectorError", "CollectorPlan", "CollectorSupervisor",
           "CollectorStatus", "validate_manifest", "render_pull_config",
           "state_from_metrics", "exporter_privileges", "StrictSupport",
           "strict_support"]
