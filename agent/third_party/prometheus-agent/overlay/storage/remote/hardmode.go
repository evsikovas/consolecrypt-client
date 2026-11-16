// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

package remote

import (
	"context"
	"errors"

	"github.com/prometheus/prometheus/tsdb/wlog"
)

// hardWriteClient fences the actual maintained client's Store, not merely
// enqueue. No remote-write wire, samples or ACK journal is introduced.
type hardWriteClient struct {
	WriteClient
	h *wlog.HardMode
}

func (c hardWriteClient) Store(parent context.Context, req []byte, retry int) (WriteResponseStats, error) {
	ctx, finish, err := c.h.BeginSend(parent)
	if err != nil {
		return WriteResponseStats{}, err
	}
	stats, err := c.WriteClient.Store(ctx, req, retry)
	if !finish() {
		return WriteResponseStats{}, wlog.ErrHardModeUnavailable
	}
	return stats, err
}

// SetAgentHardMode is initialization-only, before any config/queue is started.
func (s *Storage) SetAgentHardMode(h *wlog.HardMode) { s.rws.hard = h }
func (s *Storage) AgentHardMode() *wlog.HardMode     { return s.rws.hard }

// HardReclaim closes EVERY native reader/send generation before fn can unlink.
// Losing the in-memory batches is deliberate: surviving payload is replayed
// from the existing native WAL. Equal timestamp/value duplicates are possible.
// A failed fn leaves sends stopped and the on-disk state untouched thereafter.
func (s *Storage) HardReclaim(fn func() error) error {
	r := s.rws
	r.lifecycle.Lock()
	defer r.lifecycle.Unlock()
	if r.closed || r.hard == nil || r.hard.Failed() {
		return errors.New("hard reclaim without hard mode")
	}
	r.hard.Fence()
	if r.hard.Failed() {
		return wlog.ErrHardModeUnavailable
	}
	r.mtx.Lock()
	for _, q := range r.queues {
		q.abortHardMode()
	}
	clear(r.queues)
	cfg := r.hardConfig
	r.mtx.Unlock()
	if r.hard.Failed() {
		return wlog.ErrHardModeUnavailable
	}
	if err := fn(); err != nil {
		r.hard.Fail()
		return err
	}
	if cfg != nil {
		if err := r.applyConfig(cfg); err != nil {
			r.hard.Fail()
			return err
		}
	}
	return nil
}

func (t *QueueManager) setHardMode(h *wlog.HardMode) {
	t.hard = h
	t.storeClient = hardWriteClient{WriteClient: t.storeClient, h: h}
	t.watcher.SetAgentHardMode(h)
}

func (t *QueueManager) abortHardMode() {
	// Hard mode admits one fixed shard, so no reshard can race this cancellation.
	// Cancel BEFORE Stop's existing flushing path. Even if it attempts a Store,
	// the fenced hardWriteClient rejects before any HTTP call.
	t.shards.mtx.RLock()
	t.shards.hardShutdown()
	t.shards.mtx.RUnlock()
	t.Stop()
}
