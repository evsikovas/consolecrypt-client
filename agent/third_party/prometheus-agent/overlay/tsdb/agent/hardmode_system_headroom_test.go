// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package agent

import (
    "context"
    "errors"
    "fmt"
    "net/http"
    "net/http/httptest"
    "os"
    "path/filepath"
    "reflect"
    "sync/atomic"
    "testing"

    "github.com/prometheus/client_golang/prometheus/testutil"
    "github.com/prometheus/common/promslog"
    "github.com/prometheus/prometheus/config"
    "github.com/prometheus/prometheus/model/labels"
    "github.com/prometheus/prometheus/tsdb/wlog"
    "golang.org/x/sys/unix"
)

func TestHardModeSystemHeadroomThresholdAndOverflow(t *testing.T) {
    floor := uint64(hardSystemHeadroom)
    for _, growth := range []int64{0, 65536, 2*65536, hardCheckpoint + 65536} {
        limit := floor + uint64(growth)
        for _, delta := range []int64{-1, 0, 1} {
            available := uint64(int64(limit) + delta)
            got := hardSystemAvailableCause(available, 1, growth)
            want := ""
            if delta < 0 { want = "low_available" }
            if got != want { t.Fatalf("threshold growth=%d delta=%d got=%q want=%q", growth, delta, got, want) }
        }
    }
    invalid := []struct{blocks uint64; size, growth int64}{
        {1, 0, 0}, {1, -1, 0}, {1, 65537, 0}, {1, 4096, -1},
        {^uint64(0), 4096, 0}, {1, 4096, int64(1<<63-1)},
    }
    for _, row := range invalid {
        if hardSystemAvailableCause(row.blocks, row.size, row.growth) != "observation_invalid" { t.Fatal("invalid observation accepted") }
    }
    // CP budget is forecast before checkpoint allocation, not only before Log.
    if hardSystemAvailableCause(floor+65536, 1, hardCheckpoint+65536) != "low_available" { t.Fatal("checkpoint forecast omitted") }
}

func TestHardModeSystemHeadroomClosedCauseSticky(t *testing.T) {
    m := newDBMetrics(nil)
    h := &hardBudget{mode: wlog.NewHardMode()}
    h.diskReject = func(cause string) { m.hardDiskGuardRejections.WithLabelValues(cause).Inc() }
    if !errors.Is(h.rejectSystemDisk("not-a-public-label"), wlog.ErrHardModeUnavailable) || !h.mode.Failed() { t.Fatal("invalid observation not sticky") }
    if testutil.ToFloat64(m.hardDiskGuardRejections.WithLabelValues("observation_invalid")) != 1 ||
        testutil.ToFloat64(m.hardDiskGuardRejections.WithLabelValues("low_available")) != 0 { t.Fatal("closed counter cause mismatch") }
}

func TestCCOwnedHardmodeSystemHeadroomNearFull(t *testing.T) {
    const owned = "/cc-hardmode-headroom"
    if os.Getenv("CC_PROM_HARDMODE_HEADROOM_ROOT") == "" { t.Skip("requires explicit owned private256MiB filesystem") }
    if os.Getenv("CC_PROM_HARDMODE_HEADROOM_ROOT") != owned || os.Getuid() != 10001 { t.Fatal("invalid explicit owned headroom fixture") }
    if _, err := os.Stat("/.dockerenv"); err != nil { t.Fatal("owned container required") }
    var st unix.Stat_t
    var fs unix.Statfs_t
    if unix.Lstat(owned, &st) != nil || st.Mode&unix.S_IFMT != unix.S_IFDIR || st.Uid != 10001 || st.Mode&0o077 != 0 ||
        unix.Statfs(owned, &fs) != nil || fs.Type != unix.TMPFS_MAGIC || fs.Bsize != 4096 || fs.Blocks*uint64(fs.Bsize) != 256<<20 { t.Fatal("private filesystem geometry mismatch") }
    entries, err := os.ReadDir(owned)
    if err != nil || len(entries) != 0 { t.Fatal("private filesystem must start empty") }
    queue := filepath.Join(owned, "queue")
    if err := os.Mkdir(queue, 0o700); err != nil { t.Fatal(err) }
    defer os.RemoveAll(queue) // Only this fresh owned test queue.
    if err := os.WriteFile(filepath.Join(queue, "lock"), []byte(hardLayoutMagic), 0o600); err != nil { t.Fatal(err) }
    var requests atomic.Int32
    receiver := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { requests.Add(1); w.WriteHeader(http.StatusNoContent) }))
    defer receiver.Close()
    db, rs, err := hardTestOpen(t, queue)
    if err != nil { rs.Close(); t.Fatal(err) }
    closed := false
    defer func(){ if !closed { db.hard.mode.Fail(); db.Close(); rs.Close() } }()
    conf, err := config.Load(fmt.Sprintf(`remote_write:
- name: cc_headroom
  url: %s/api/v1/write
  follow_redirects: false
  proxy_from_environment: false
  metadata_config:
    send: false
  queue_config:
    capacity: 128
    min_shards: 1
    max_shards: 1
    min_backoff: 100ms
    max_backoff: 1s
    sample_age_limit: 0s
`, receiver.URL), promslog.NewNopLogger())
    if err != nil { t.Fatal(err) }
    if err := rs.ApplyConfig(conf); err != nil { t.Fatal(err) }
    db.SetWriteNotified(rs)
    filler, err := os.OpenFile(filepath.Join(owned, "outside-wal-filler"), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
    if err != nil { t.Fatal(err) }
    defer os.Remove(filler.Name())
    defer filler.Close()
    buf := make([]byte, 65536)
    written := 0
    for {
        if unix.Statfs(owned, &fs) != nil { t.Fatal("available observation failed") }
        available := fs.Bavail*uint64(fs.Bsize)
        if available < uint64(hardSystemHeadroom) { break }
        if written >= 256<<20 { t.Fatal("bounded filler exhausted") }
        count, err := filler.Write(buf)
        if err != nil || count != len(buf) { t.Fatal("filler reached ENOSPC instead of headroom threshold", err) }
        written += count
    }
    available := fs.Bavail*uint64(fs.Bsize)
    if available+65536 < uint64(hardSystemHeadroom) { t.Fatal("filler overshot one allocation quantum") }
    before := hardObserveWholeRoot(t, queue)
    a := db.Appender(context.Background()).(*appender)
    if _, err := a.Append(0, labels.FromStrings("__name__", "cc_headroom"), 1, 17.25); err != nil { t.Fatal(err) }
    if err := a.Commit(); !errors.Is(err, wlog.ErrHardModeUnavailable) { t.Fatal("native pre-write guard did not reject", err) }
    if !db.hard.mode.Failed() || testutil.ToFloat64(db.metrics.hardDiskGuardRejections.WithLabelValues("low_available")) != 1 { t.Fatal("low available not sticky/counted") }
    if requests.Load() != 0 { t.Fatal("rejected batch reached HTTP") }
    if !reflect.DeepEqual(before, hardObserveWholeRoot(t, queue)) { t.Fatal("headroom rejection mutated WAL root") }
    if db.Close() == nil { t.Fatal("failed pipeline close reported success") }
    if err := rs.Close(); err != nil { t.Fatal(err) }
    closed = true
    if !reflect.DeepEqual(before, hardObserveWholeRoot(t, queue)) { t.Fatal("failed close padded/repaired WAL") }
    reopened, r2, err := hardTestOpen(t, queue)
    if reopened != nil { reopened.Close() }
    r2.Close()
    if err == nil || reopened != nil { t.Fatal("near-full startup admitted NewSize") }
    if !reflect.DeepEqual(before, hardObserveWholeRoot(t, queue)) { t.Fatal("near-full startup mutated existing WAL/guard") }
    t.Logf("real private256MiB filesystem: available=%d; prod128MiB floor; outsideWAL filler=%d; rejected/noHTTP/noWALwrite; noOSreserve/shared-writer guarantee", available, written)
}
