// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

package wlog

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/prometheus/prometheus/model/labels"
	"github.com/prometheus/prometheus/tsdb/record"
	"github.com/prometheus/prometheus/util/compression"
)

func TestHardModeFenceJoinsAndLateAckCannotRevive(t *testing.T) {
	h := NewHardMode()
	if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	ctx, finish, err := h.BeginSend(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	go func() { h.Fence(); close(done) }()
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("request not cancelled")
	}
	select {
	case <-done:
		t.Fatal("returned credit before Store completed")
	default:
	}
	if finish() {
		t.Fatal("late ACK became delivery")
	}
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("fence did not join")
	}
	if _, _, err := h.BeginSend(context.Background()); err == nil {
		t.Fatal("fenced generation admitted retry")
	}
	if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	_, release, err := h.BeginSend(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !release() {
		t.Fatal("fresh generation unavailable")
	}
}

func TestHardModeClockRollbackAndExpiryAreSticky(t *testing.T) {
	h := NewHardMode()
	if err := h.Resume(time.Now().Add(-time.Second)); err == nil {
		t.Fatal("expired generation")
	}
	h = NewHardMode()
	if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	h.mu.Lock()
	valid := h.observe(time.Now().Add(-time.Hour))
	h.mu.Unlock()
	if valid || !h.Failed() || h.Ready() {
		t.Fatal("rollback revived generation")
	}
	if err := h.Resume(time.Now().Add(time.Hour)); err == nil {
		t.Fatal("failed generation reset")
	}
}

func TestHardModeCancelledParentCannotBecomeAck(t *testing.T) {
	h := NewHardMode()
	if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	parent, cancel := context.WithCancel(context.Background())
	_, finish, err := h.BeginSend(parent)
	if err != nil {
		t.Fatal(err)
	}
	cancel()
	if finish() {
		t.Fatal("cancelled native sender became a positive ACK")
	}
	if !h.Ready() {
		t.Fatal("unrelated live generation was destroyed")
	}
	h.Fence()
}

type hardReplaySink struct{ samples []record.RefSample }

func (s *hardReplaySink) Append(v []record.RefSample) bool {
	s.samples = append(s.samples, v...)
	return true
}
func (*hardReplaySink) AppendExemplars([]record.RefExemplar) bool                   { return true }
func (*hardReplaySink) AppendHistograms([]record.RefHistogramSample) bool           { return true }
func (*hardReplaySink) AppendFloatHistograms([]record.RefFloatHistogramSample) bool { return true }
func (*hardReplaySink) StoreSeries([]record.RefSeries, int)                         {}
func (*hardReplaySink) StoreMetadata([]record.RefMetadata)                          {}
func (*hardReplaySink) UpdateSeriesSegment([]record.RefSeries, int)                 {}
func (*hardReplaySink) SeriesReset(int)                                             {}

func TestHardModeHistoricalReplayNeverSendsCheckpointZero(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "wal")
	w, err := NewSize(nil, nil, dir, 10*1024*1024, compression.None)
	if err != nil {
		t.Fatal(err)
	}
	w.SetSynchronousRotation()
	if _, err = w.NextSegmentSync(); err != nil {
		t.Fatal(err)
	}
	enc := record.Encoder{}
	series := []record.RefSeries{{Ref: 1, Labels: labels.FromStrings("__name__", "historical")}}
	future := time.Now().Add(24 * time.Hour).UnixMilli()
	if err = w.Log(enc.Series(series, nil), enc.Samples([]record.RefSample{{Ref: 1, T: -100000, V: 17}, {Ref: 1, T: future, V: 42}}, nil)); err != nil {
		t.Fatal(err)
	}
	if err = w.Close(); err != nil {
		t.Fatal(err)
	}
	cpDir := CheckpointDir(dir, 0)
	cp, err := NewSize(nil, nil, cpDir, 10*1024*1024, compression.None)
	if err != nil {
		t.Fatal(err)
	}
	if err = cp.Log(enc.Series(series, nil), enc.Samples([]record.RefSample{{Ref: 1, T: future, V: 0}}, nil)); err != nil {
		t.Fatal(err)
	}
	if err = cp.Close(); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(SegmentName(dir, 0)); err != nil {
		t.Fatal(err)
	}
	sink := &hardReplaySink{}
	watcher := NewWatcher(NewWatcherMetrics(nil), NewLiveReaderMetrics(nil), nil, "hard-test", sink, root, false, false, false, nil)
	watcher.SetMetrics()
	watcher.SetAgentHardMode(NewHardMode())
	watcher.SetStartTime(time.Now())
	if err = watcher.readCheckpoint(cpDir, (*Watcher).readSegment); err != nil {
		t.Fatal(err)
	}
	if len(sink.samples) != 0 {
		t.Fatal("checkpoint sentinel escaped as payload")
	}
	if err = watcher.watch(1, true); err != nil {
		t.Fatal(err)
	}
	if len(sink.samples) != 2 || sink.samples[0].T != -100000 || sink.samples[0].V != 17 || sink.samples[1].T != future || sink.samples[1].V != 42 {
		t.Fatal("historical/future original values or timestamps lost")
	}
}
