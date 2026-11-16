// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0

package wlog

import (
	"context"
	"errors"
	"sync"
	"time"
)

// ErrHardModeUnavailable deliberately says nothing about remote acceptance.
// A cancelled request may already have been accepted by the receiver.
var ErrHardModeUnavailable = errors.New("agent hard budget unavailable")

// HardMode is the shared *native* Agent WAL/send generation. It contains no
// samples, ACKs, durable index or sample codec. Reclamation cancels and joins
// the entire generation, not just the requests known to contain old samples.
// The deadline is conservative for every sample: it is derived from the oldest
// retained payload segment, never from a source timestamp.
type HardMode struct {
	mu       sync.Mutex
	changed  *sync.Cond
	ctx      context.Context
	cancel   context.CancelFunc
	deadline time.Time
	high     time.Time
	active   int
	open     bool
	failed   bool
}

func NewHardMode() *HardMode {
	h := &HardMode{}
	h.changed = sync.NewCond(&h.mu)
	return h
}

// Resume is called only with all old native queues/watchers joined and after
// the Agent has checked the surviving native files. Failure is sticky.
func (h *HardMode) Resume(deadline time.Time) error {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.failed || h.open || h.active != 0 || !h.observe(time.Now()) || !time.Now().Before(deadline) {
		return ErrHardModeUnavailable
	}
	h.ctx, h.cancel = context.WithDeadline(context.Background(), deadline)
	h.deadline = deadline
	h.open = true
	h.changed.Broadcast()
	return nil
}

// observe compares wall observations as well as retaining time.Time's monotonic
// deadline component. It must be called with mu held. A backwards OS clock is
// unavailable, not an extension of residence.
func (h *HardMode) observe(now time.Time) bool {
	if now.UnixNano() <= 0 || (!h.high.IsZero() && now.UnixNano() < h.high.UnixNano()) {
		h.failed = true
		if h.cancel != nil {
			h.cancel()
		}
		h.open = false
		return false
	}
	h.high = now
	return true
}

func (h *HardMode) Ready() bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return !h.failed && h.open && h.observe(time.Now()) && time.Now().Before(h.deadline)
}

// RetryDone projects the currently open native generation without admitting a
// Store or changing its deadline. A retry loop captures this once: Resume must
// never revive a wait belonging to an older, already cancelled generation.
func (h *HardMode) RetryDone() (<-chan struct{}, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.open || h.failed || !h.observe(time.Now()) || !time.Now().Before(h.deadline) {
		return nil, ErrHardModeUnavailable
	}
	return h.ctx.Done(), nil
}

func (h *HardMode) Failed() bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.failed
}

// Fail cancels, but does not join: watcher callers must never wait for their
// own completion. The Agent's reclamation/close path performs Fence and joins.
func (h *HardMode) Fail() {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.failed = true
	h.open = false
	if h.cancel != nil {
		h.cancel()
	}
	h.changed.Broadcast()
}

// Fence denies new Store calls BEFORE cancellation and waits for all existing
// Store calls to return. It must not be called while holding a sender lock.
// A broken transport that ignores context prevents reclamation; it does not
// authorize unlinking or returning physical budget credit.
func (h *HardMode) Fence() {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.open = false
	if h.cancel != nil {
		h.cancel()
	}
	for h.active != 0 {
		h.changed.Wait()
	}
}

// BeginSend admits exactly one existing native WriteClient.Store invocation.
// finish MUST run after Store, including on errors. Retrying takes a fresh
// admission; a cancelled generation cannot be revived by a late ACK.
func (h *HardMode) BeginSend(parent context.Context) (context.Context, func() bool, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.open || h.failed || !h.observe(time.Now()) || !time.Now().Before(h.deadline) || parent.Err() != nil {
		return nil, nil, ErrHardModeUnavailable
	}
	generation := h.ctx
	ctx, cancel := context.WithCancel(parent)
	stop := context.AfterFunc(generation, cancel)
	h.active++
	return ctx, func() bool {
		uncancelled := parent.Err() == nil && ctx.Err() == nil
		stop()
		cancel()
		h.mu.Lock()
		defer h.mu.Unlock()
		valid := uncancelled && h.observe(time.Now()) && h.open && !h.failed && generation == h.ctx && generation.Err() == nil && time.Now().Before(h.deadline)
		h.active--
		h.changed.Broadcast()
		return valid
	}, nil
}
