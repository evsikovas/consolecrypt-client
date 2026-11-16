// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package agent

import (
	"context"
	"github.com/prometheus/client_golang/prometheus/testutil"
	"github.com/prometheus/common/promslog"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/prometheus/prometheus/model/labels"
	"github.com/prometheus/prometheus/model/metadata"
	"github.com/prometheus/prometheus/storage/remote"
	"github.com/prometheus/prometheus/tsdb/record"
	"github.com/prometheus/prometheus/tsdb/wlog"
	"github.com/prometheus/prometheus/util/compression"
)

func hardTestOwnedRoot(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	// Explicit fixture-only initialization; production Open NEVER creates this.
	if err := os.WriteFile(filepath.Join(root, "lock"), []byte(hardLayoutMagic), 0o600); err != nil {
		t.Fatal(err)
	}
	return root
}

func hardTestOpen(t *testing.T, root string) (*DB, *remote.Storage, error) {
	t.Helper()
	r := remote.NewStorage(nil, nil, nil, root, time.Second, nil, false)
	r.SetAgentHardMode(wlog.NewHardMode())
	o := DefaultOptions()
	o.HardBudget = true
	o.WALCompression = compression.None
	db, err := Open(promslog.NewNopLogger(), nil, r, root, o)
	return db, r, err
}

func TestHardModeNativePressureCheckpointAndRestart(t *testing.T) {
	root := hardTestOwnedRoot(t)
	db, r, err := hardTestOpen(t, root)
	if err != nil {
		r.Close()
		t.Fatal(err)
	}
	closed := false
	defer func() {
		if !closed {
			db.Close()
			r.Close()
		}
	}()
	db.hardMu.Lock()
	db.hard.liveLimit = 2 << 20
	db.hardMu.Unlock()
	base := time.Now().Add(24 * time.Hour).UnixMilli()
	const batches = 90
	for b := 0; b < batches; b++ {
		a := db.Appender(context.Background())
		for i := 0; i < 4096; i++ {
			v := b*4096 + i
			if _, err := a.Append(0, labels.FromStrings("__name__", "hard_signal"), base+int64(v), float64(v)+0.25); err != nil {
				a.Rollback()
				t.Fatal(err)
			}
		}
		if err := a.Commit(); err != nil {
			t.Fatal(err)
		}
	}
	db.hardMu.Lock()
	inv, err := db.hard.inventory(false)
	db.hardMu.Unlock()
	if err != nil || inv.charged+hardReserve > hardPhysical {
		t.Fatal("physical reservation failed", err)
	}
	first, _, err := wlog.Segments(db.wal.Dir())
	if err != nil || first <= 0 {
		t.Fatal("pressure did not evict oldest native prefix", err)
	}
	if _, _, err = wlog.LastCheckpoint(db.wal.Dir()); err != nil {
		t.Fatal("missing native metadata checkpoint", err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	r.Close()
	closed = true
	// Enumerate ORIGINAL records; no instant-vector/current-value substitute.
	sr, err := wlog.NewSegmentsReader(filepath.Join(root, "wal"))
	if err != nil {
		t.Fatal(err)
	}
	reader := wlog.NewReader(sr)
	dec := record.NewDecoder(labels.NewSymbolTable(), promslog.NewNopLogger())
	last := int64(-1)
	firstOrdinal := int64(-1)
	count := 0
	for reader.Next() {
		rec := reader.Record()
		typ := dec.Type(rec)
		if typ == record.Samples || typ == record.SamplesV2 {
			samples, err := dec.Samples(rec, nil)
			if err != nil {
				t.Fatal(err)
			}
			for _, s := range samples {
				ordinal := s.T - base
				if ordinal < 0 || s.V != float64(ordinal)+0.25 {
					t.Fatal("native timestamp/value rewritten")
				}
				if count == 0 {
					firstOrdinal = ordinal
				} else if s.T != last+1 {
					t.Fatal("retained native suffix has a hole or duplicate")
				}
				last = s.T
				count++
			}
		}
	}
	if err = reader.Err(); err != nil && err != io.EOF {
		t.Fatal(err)
	}
	sr.Close()
	if count == 0 || count >= batches*4096 || last != base+batches*4096-1 || int64(count) != last-base-firstOrdinal+1 {
		t.Fatal("wrong retained historical suffix")
	}
	db2, r2, err := hardTestOpen(t, root)
	if err != nil {
		r2.Close()
		t.Fatal("restart rejected valid native tail", err)
	}
	defer r2.Close()
	defer db2.Close()
}

func TestHardModeUnknownAndCorruptRecoveryDoesNotRepair(t *testing.T) {
	for _, foreign := range []bool{true, false} {
		root := hardTestOwnedRoot(t)
		wal := filepath.Join(root, "wal")
		if err := os.Mkdir(wal, 0o700); err != nil {
			t.Fatal(err)
		}
		name := "00000000"
		if foreign {
			name = "foreign"
		}
		p := filepath.Join(wal, name)
		before := []byte("not a valid native WAL record")
		if err := os.WriteFile(p, before, 0o600); err != nil {
			t.Fatal(err)
		}
		db, r, err := hardTestOpen(t, root)
		if db != nil {
			db.Close()
		}
		r.Close()
		if err == nil {
			t.Fatal("unknown/corrupt recovery was adopted")
		}
		after, readErr := os.ReadFile(p)
		if readErr != nil || string(after) != string(before) {
			t.Fatal("corrupt original was repaired")
		}
		entries, err := os.ReadDir(wal)
		if err != nil || len(entries) != 1 || entries[0].Name() != name {
			t.Fatal("recovery wrote a new WAL generation")
		}
	}
}

func TestHardModeFutureMtimeAndLinkedPayloadUnavailable(t *testing.T) {
	for _, linked := range []bool{true, false} {
		root := hardTestOwnedRoot(t)
		wal := filepath.Join(root, "wal")
		if err := os.Mkdir(wal, 0o700); err != nil {
			t.Fatal(err)
		}
		p := filepath.Join(wal, "00000000")
		if err := os.WriteFile(p, nil, 0o600); err != nil {
			t.Fatal(err)
		}
		if linked {
			if err := os.Link(p, filepath.Join(t.TempDir(), "own-link")); err != nil {
				t.Fatal(err)
			}
		} else {
			future := time.Now().Add(time.Hour)
			if err := os.Chtimes(p, future, future); err != nil {
				t.Fatal(err)
			}
		}
		db, r, err := hardTestOpen(t, root)
		if db != nil {
			db.Close()
		}
		r.Close()
		if err == nil {
			t.Fatal("unknown residence/link accepted")
		}
	}
}

func TestHardModeRollbackSeriesAndUnsupportedProfile(t *testing.T) {
	root := hardTestOwnedRoot(t)
	db, r, err := hardTestOpen(t, root)
	if err != nil {
		r.Close()
		t.Fatal(err)
	}
	defer r.Close()
	defer db.Close()
	a := db.Appender(context.Background())
	if _, err := a.Append(0, labels.FromStrings("__name__", "rolled_back"), 1, 3); err != nil {
		t.Fatal(err)
	}
	if err := a.Rollback(); err != nil {
		t.Fatal("rollback series log bypassed budget", err)
	}
	base := &appenderBase{DB: db, pendingSamples: make([]record.RefSample, 4097)}
	_, before, err := db.wal.LastSegmentAndOffset()
	if err != nil {
		t.Fatal(err)
	}
	if err = base.log(); err == nil {
		t.Fatal("oversized admitted profile")
	}
	_, after, err := db.wal.LastSegmentAndOffset()
	if err != nil || after != before {
		t.Fatal("rejected record wrote payload", err)
	}
}

func TestHardModeResidenceIndependentOfSourceTimestamp(t *testing.T) {
	for _, sourceTime := range []int64{1, time.Now().Add(24 * time.Hour).UnixMilli()} {
		root := hardTestOwnedRoot(t)
		db, r, err := hardTestOpen(t, root)
		if err != nil {
			r.Close()
			t.Fatal(err)
		}
		a := db.Appender(context.Background())
		if _, err = a.Append(0, labels.FromStrings("__name__", "physical_age"), sourceTime, 123.5); err != nil {
			t.Fatal(err)
		}
		if err = a.Commit(); err != nil {
			t.Fatal(err)
		}
		func() {
			db.hardMu.Lock()
			defer db.hardMu.Unlock()
			if err = db.hardMaintain(0); err != nil {
				t.Fatal(err)
			}
			first, _, err := wlog.Segments(db.wal.Dir())
			if err != nil || first != 0 {
				t.Fatal("source timestamp incorrectly expired fresh data")
			}
			if _, err = db.wal.NextSegmentSync(); err != nil {
				t.Fatal(err)
			}
			old := time.Now().Add(-116 * time.Minute)
			if err = os.Chtimes(wlog.SegmentName(db.wal.Dir(), 0), old, old); err != nil {
				t.Fatal(err)
			}
			if err = db.hardMaintain(0); err != nil {
				t.Fatal(err)
			}
			first, _, err = wlog.Segments(db.wal.Dir())
			if err != nil || first != 1 {
				t.Fatal("expired residence retained because source was future")
			}
			if testutil.ToFloat64(db.metrics.hardAgeEvicted) != 1 || testutil.ToFloat64(db.metrics.hardByteEvicted) != 0 {
				t.Fatal("residence cause conflated with byte pressure")
			}
		}()
		db.Close()
		r.Close()
	}
}

func TestHardModeConcurrentNativeCommits(t *testing.T) {
	root := hardTestOwnedRoot(t)
	db, r, err := hardTestOpen(t, root)
	if err != nil {
		r.Close()
		t.Fatal(err)
	}
	defer r.Close()
	defer db.Close()
	var wg sync.WaitGroup
	failures := make(chan error, 4)
	for worker := 0; worker < 4; worker++ {
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			l := labels.FromStrings("__name__", "concurrent", "worker", string(rune('a'+n)))
			for b := 0; b < 60; b++ {
				a := db.Appender(context.Background())
				for i := 0; i < 128; i++ {
					if _, err := a.Append(0, l, int64(b*128+i+1), float64(i)); err != nil {
						a.Rollback()
						failures <- err
						return
					}
				}
				if err := a.Commit(); err != nil {
					failures <- err
					return
				}
			}
		}(worker)
	}
	wg.Wait()
	close(failures)
	for err := range failures {
		t.Fatal(err)
	}
	db.hardMu.Lock()
	defer db.hardMu.Unlock()
	inv, err := db.hard.inventory(false)
	if err != nil || inv.charged+hardReserve > hardPhysical {
		t.Fatal("concurrent physical bound", err)
	}
}

func TestHardModeHeldBatchCannotRefreshResidence(t *testing.T) {
	for _, rollback := range []bool{false, true} {
		root := hardTestOwnedRoot(t)
		db, r, err := hardTestOpen(t, root)
		if err != nil {
			r.Close()
			t.Fatal(err)
		}
		func() {
			defer r.Close()
			defer db.Close()
			now := time.Now()
			db.hardMu.Lock()
			db.hard.nowTest = func() time.Time { return now }
			db.hardMu.Unlock()
			a := db.Appender(context.Background()).(*appender)
			if _, err := a.Append(0, labels.FromStrings("__name__", "held"), 1, 11); err != nil {
				t.Fatal(err)
			}
			_, before, err := db.wal.LastSegmentAndOffset()
			if err != nil {
				t.Fatal(err)
			}
			db.hardMu.Lock()
			now = now.Add(61 * time.Second)
			db.hardMu.Unlock()
			if rollback {
				err = a.Rollback()
			} else {
				err = a.Commit()
			}
			if err == nil || !db.hard.mode.Failed() {
				t.Fatal("held pending data refreshed arrival on commit")
			}
			_, after, err := db.wal.LastSegmentAndOffset()
			if err != nil || after != before {
				t.Fatal("expired batch grew native WAL")
			}
			if !a.hardFirst.IsZero() || len(a.pendingSeries)+len(a.pendingSamples) != 0 {
				t.Fatal("pooled appender retained uncertain pending state")
			}
		}()
	}
	// An existing-reference metadata-only batch must capture its own first
	// observation; it does not pass through getOrCreate.
	func() {
		root := hardTestOwnedRoot(t)
		db, r, err := hardTestOpen(t, root)
		if err != nil {
			r.Close()
			t.Fatal(err)
		}
		defer r.Close()
		defer db.Close()
		l := labels.FromStrings("__name__", "metadata_delay")
		initial := db.Appender(context.Background())
		ref, err := initial.Append(0, l, 1, 7)
		if err != nil {
			t.Fatal(err)
		}
		if err = initial.Commit(); err != nil {
			t.Fatal(err)
		}
		now := time.Now()
		db.hardMu.Lock()
		db.opts.EnableMetadataWALRecords = true
		db.hard.nowTest = func() time.Time { return now }
		db.hardMu.Unlock()
		a := db.Appender(context.Background()).(*appender)
		if _, err = a.UpdateMetadata(ref, l, metadata.Metadata{Unit: "seconds", Help: "owned metadata"}); err != nil {
			t.Fatal(err)
		}
		if a.hardFirst.IsZero() {
			t.Fatal("metadata bypassed first observation")
		}
		_, before, err := db.wal.LastSegmentAndOffset()
		if err != nil {
			t.Fatal(err)
		}
		db.hardMu.Lock()
		now = now.Add(61 * time.Second)
		db.hardMu.Unlock()
		if err = a.Commit(); err == nil || !db.hard.mode.Failed() {
			t.Fatal("held metadata batch refreshed arrival")
		}
		_, after, err := db.wal.LastSegmentAndOffset()
		if err != nil || before != after {
			t.Fatal("held metadata grew WAL")
		}
	}()
	// Unmodified real OS clock/fresh batch control, no shifted-time acceptance.
	root := hardTestOwnedRoot(t)
	db, r, err := hardTestOpen(t, root)
	if err != nil {
		r.Close()
		t.Fatal(err)
	}
	defer r.Close()
	defer db.Close()
	a := db.Appender(context.Background()).(*appender)
	if _, err = a.Append(0, labels.FromStrings("__name__", "fresh"), 1, 12); err != nil {
		t.Fatal(err)
	}
	if err = a.Commit(); err != nil || !a.hardFirst.IsZero() || db.hard.mode.Failed() {
		t.Fatal("fresh normal commit rejected or retained batch epoch", err)
	}
}

func TestHardModeAggregateNativePreencodeBeforeAnyLog(t *testing.T) {
	for _, metadataOnly := range []bool{false, true} {
		root := hardTestOwnedRoot(t)
		db, r, err := hardTestOpen(t, root)
		if err != nil {
			r.Close()
			t.Fatal(err)
		}
		func() {
			defer r.Close()
			defer db.Close()
			a := &appenderBase{DB: db}
			db.hardMu.Lock()
			err := a.hardStartLocked()
			db.hardMu.Unlock()
			if err != nil {
				t.Fatal(err)
			}
			for i := 0; i < 256; i++ {
				if metadataOnly {
					a.pendingMetadata = append(a.pendingMetadata, record.RefMetadata{Ref: 1, Unit: strings.Repeat("u", 8000)})
				} else {
					a.pendingSeries = append(a.pendingSeries, record.RefSeries{Ref: 1, Labels: labels.FromStrings("__name__", strings.Repeat("x", 8000))})
				}
			}
			_, before, err := db.wal.LastSegmentAndOffset()
			if err != nil {
				t.Fatal(err)
			}
			if err = a.commit(); err == nil || !db.hard.mode.Failed() {
				t.Fatal("oversized aggregate encoded after partial WAL writes")
			}
			_, after, err := db.wal.LastSegmentAndOffset()
			if err != nil || after != before {
				t.Fatal("aggregate preflight wrote a partial transaction")
			}
			if !a.hardFirst.IsZero() || len(a.pendingSeries)+len(a.pendingMetadata) != 0 {
				t.Fatal("rejected aggregate survived pool reset")
			}
		}()
	}
}
