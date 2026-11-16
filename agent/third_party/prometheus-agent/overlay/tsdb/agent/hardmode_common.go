// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

package agent

import (
	"errors"
	"github.com/prometheus/prometheus/model/labels"
	"github.com/prometheus/prometheus/tsdb/record"
	"github.com/prometheus/prometheus/tsdb/wlog"
	"time"
)

func (h *hardBudget) closeWAL(w *wlog.WL) error {
	if h.mode.Failed() {
		return errors.Join(wlog.ErrHardModeUnavailable, w.CloseWithoutFlush())
	}
	if err := h.guard.check(); err != nil {
		h.mode.Fail()
		return errors.Join(err, w.CloseWithoutFlush())
	}
	if err := h.checkSystemHeadroom(65536); err != nil {
		return errors.Join(err, w.CloseWithoutFlush())
	}
	err := w.Close()
	post := h.guard.check()
	if err != nil || post != nil {
		h.mode.Fail()
	}
	return errors.Join(err, post)
}

const (
	hardSystemHeadroom = int64(128 << 20) // Filesystem Bavail floor; distinct from arithmetic queue reserve.
	hardPhysical   = int64(512 << 20)
	hardLive       = int64(64 << 20)
	hardCheckpoint = int64(64 << 20)
	hardReserve    = int64(128 << 20)
	hardRecord     = 1 << 20
	hardSeries     = 256
	hardRotation   = time.Minute
	hardResidence  = 115 * time.Minute
	hardBatchDelay = time.Minute
)

func (a *appenderBase) hardReject() error {
	a.metrics.hardUnavailable.Set(1)
	a.metrics.hardLossUnknown.Set(1)
	a.hard.mode.Fail()
	return wlog.ErrHardModeUnavailable
}

// Called with hardMu held, before the first real pending-data mutation.
// This is ephemeral admission timing, NOT a new persistent arrival journal.
func (a *appenderBase) hardStartLocked() error {
	n, err := a.hard.now()
	if err != nil {
		return a.hardReject()
	}
	if a.hardFirst.IsZero() {
		a.hardFirst = n
	}
	return a.hardBatchLive()
}

func (a *appenderBase) hardBatchLive() error {
	n, err := a.hard.now()
	if err != nil {
		return a.hardReject()
	}
	if a.hardFirst.IsZero() {
		if len(a.pendingSeries)+len(a.pendingSamples)+len(a.pendingMetadata) > 0 {
			return a.hardReject()
		}
		return nil
	}
	if n.UnixNano() < a.hardFirst.UnixNano() || !n.Before(a.hardFirst.Add(hardBatchDelay)) {
		return a.hardReject()
	}
	return nil
}

// This is an explicit bounded scalar telemetry profile, not silent support for
// arbitrary Prometheus ingest. Unsupported records/new series return an error.
// Existing pending data is never secretly redirected to another queue.
func (a *appenderBase) hardPendingBound() error {
	if a.hard.mode.Failed() || len(a.pendingSamples) > 4096 || len(a.pendingSeries) > hardSeries || len(a.pendingMetadata) > hardSeries || len(a.pendingHistograms)+len(a.pendingFloatHistograms)+len(a.pendingExamplars) != 0 {
		return a.hardReject()
	}
	for _, s := range a.pendingSeries {
		if !hardLabelBound(s.Labels) {
			return a.hardReject()
		}
	}
	for _, m := range a.pendingMetadata {
		if len(m.Unit)+len(m.Help) > 8192 {
			return a.hardReject()
		}
	}
	// Size ALL aggregate native records before the first Log. Individually valid
	// 256x8KiB labels/metadata must not cause a later partial transaction write.
	enc := record.Encoder{EnableSTStorage: a.opts.EnableSTStorage}
	for _, buf := range [][]byte{enc.Series(a.pendingSeries, nil), enc.Metadata(a.pendingMetadata, nil), enc.Samples(a.pendingSamples, nil)} {
		if len(buf) > hardRecord {
			return a.hardReject()
		}
	}
	return a.hardBatchLive()
}

func hardLabelBound(l labels.Labels) bool {
	enc := record.Encoder{}
	return len(enc.Series([]record.RefSeries{{Labels: l}}, nil)) <= 8192
}

// The enclosing log/logSeries call owns hardMu. Native encoding, framing,
// compression and physical writes remain in the maintained WAL implementation.
func (a *appenderBase) writeAgentRecord(buf []byte) error {
	if a.hard == nil {
		return a.wal.Log(buf)
	}
	if len(buf) > hardRecord {
		return a.hardReject()
	}
	if err := a.hardMaintain(int64(2*len(buf) + 65536)); err != nil {
		a.hardReject()
		return err
	}
	if err := a.hardBatchLive(); err != nil {
		return err
	}
	if err := a.hard.guard.check(); err != nil {
		a.hardReject()
		return err
	}
	if err := a.hard.checkSystemHeadroom(int64(2*len(buf) + 65536)); err != nil {
		return err
	}
	if err := a.wal.Log(buf); err != nil {
		a.hardReject()
		return err
	}
	if err := a.hardBatchLive(); err != nil {
		return err
	}
	inv, err := a.hard.inventory(false)
	if err != nil {
		a.hardReject()
		return err
	}
	a.metrics.hardAllocated.Set(float64(inv.charged))
	return nil
}
