// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package agent

import (
    "context"
    "fmt"
    "io"
    "log/slog"
    "net"
    "os"
    "path/filepath"
    "reflect"
    "strings"
    "sync"
    "testing"
    "time"

    "github.com/prometheus/client_golang/prometheus/testutil"
    "github.com/prometheus/prometheus/config"
    "github.com/prometheus/prometheus/model/labels"
    "github.com/prometheus/prometheus/storage/remote"
    "github.com/prometheus/prometheus/tsdb/record"
    "github.com/prometheus/prometheus/tsdb/wlog"
    "github.com/prometheus/prometheus/util/compression"
)

// TEST-ONLY bounded diagnostics. No payloads, credentials or external URLs are
// supplied by this fixture. All paths/labels/ports are its own disposable data.
type ccReclaimLog struct { mu sync.Mutex; data []byte; overflow bool }
func (l *ccReclaimLog) Write(p []byte) (int, error) {
    l.mu.Lock(); defer l.mu.Unlock()
    originalLength:=len(p)
    left := 65536-len(l.data)
    if len(p)>left { l.overflow=true; p=p[:left] }
    l.data=append(l.data,p...)
    // The logging sink must never change the production execution outcome.
    return originalLength,nil
}
func (l *ccReclaimLog) print(t *testing.T) {
    l.mu.Lock(); defer l.mu.Unlock()
    t.Logf("bounded_error_log overflow=%v bytes=%d\n%s",l.overflow,len(l.data),string(l.data))
}

func ccReclaimSamples(dir string, first int) ([]record.RefSample,error) {
    source,err:=wlog.NewSegmentsRangeReader(wlog.SegmentRange{Dir:dir,First:first,Last:-1})
    if err!=nil { return nil,err }
    closed:=false
    defer func(){if !closed {source.Close()}}()
    reader:=wlog.NewReader(source)
    decoder:=record.NewDecoder(labels.NewSymbolTable(),slog.New(slog.NewTextHandler(io.Discard,nil)))
    var result []record.RefSample
    for reader.Next() {
        raw:=reader.Record()
        if decoder.Type(raw)!=record.Samples && decoder.Type(raw)!=record.SamplesV2 { continue }
        samples,err:=decoder.Samples(raw,nil)
        if err!=nil { return nil,err }
        result=append(result,samples...)
    }
    if err:=reader.Err();err!=nil && err!=io.EOF { return nil,err }
    closeErr:=source.Close();closed=true
    if closeErr!=nil { return nil,closeErr }
    return result,nil
}

func ccReclaimConfiguredRemote(t *testing.T, rs *remote.Storage, logger *slog.Logger) {
    t.Helper()
    listener,err:=net.Listen("tcp","127.0.0.1:0")
    if err!=nil { t.Fatal(err) }
    port:=listener.Addr().(*net.TCPAddr).Port
    if err=listener.Close();err!=nil { t.Fatal(err) }
    // Actual native HTTP/RW path to an unavailable owned loopback port. This
    // is not a receiver, encoder, proxy or fake successful Store implementation.
    text:=fmt.Sprintf(`remote_write:
- name: cc_reclaim_regression
  url: http://127.0.0.1:%d/api/v1/write
  remote_timeout: 1s
  follow_redirects: false
  proxy_from_environment: false
  enable_http2: false
  send_exemplars: false
  send_native_histograms: false
  metadata_config:
    send: false
  queue_config:
    capacity: 1024
    min_shards: 1
    max_shards: 1
    max_samples_per_send: 256
    batch_send_deadline: 1s
    min_backoff: 100ms
    max_backoff: 1s
    sample_age_limit: 0s
`,port)
    conf,err:=config.Load(text,logger)
    if err!=nil { t.Fatal(err) }
    if err=rs.ApplyConfig(conf);err!=nil { t.Fatal(err) }
}

// A causal UNIT regression, NOT AC19 acceptance: the only aging is explicit
// Chtimes of one owned CLOSED native segment. No production/OS clock is faked,
// no 2h/3h outage is claimed, no queue file is fabricated or removed by a test.
func TestCCOwnedReclaimAfterByteCutEmptyRotationsAndExpiredGeneration(t *testing.T) {
    for _,configured:=range []bool{false,true} {
        name:="without_remote_queue";if configured {name="with_expired_remote_queue"}
        t.Run(name,func(t *testing.T) {
            sink:=&ccReclaimLog{}
            logger:=slog.New(slog.NewTextHandler(sink,&slog.HandlerOptions{Level:slog.LevelError}))
            defer sink.print(t)
            root:=t.TempDir()
            if err:=os.WriteFile(filepath.Join(root,"lock"),[]byte(hardLayoutMagic),0o600);err!=nil {t.Fatal(err)}
            rs:=remote.NewStorage(logger,nil,nil,root,time.Second,nil,false)
            rs.SetAgentHardMode(wlog.NewHardMode())
            options:=DefaultOptions();options.HardBudget=true;options.WALCompression=compression.None
            db,err:=Open(logger,nil,rs,root,options)
            if err!=nil {rs.Close();t.Fatal(err)}
            defer func(){
                // Unchanged native primary-before-secondary shutdown order.
                if err:=db.Close();err!=nil {t.Errorf("healthy native DB cleanup: %v",err)}
                if err:=rs.Close();err!=nil {t.Errorf("native remote cleanup: %v",err)}
            }()
            db.SetWriteNotified(rs)
            if configured {ccReclaimConfiguredRemote(t,rs,logger)}
            db.hardMu.Lock();db.hard.liveLimit=2<<20;db.hardMu.Unlock()
            base:=time.Now().UnixMilli()
            for batch:=0;batch<90 && testutil.ToFloat64(db.metrics.hardByteEvicted)==0;batch++ {
                appender:=db.Appender(context.Background())
                for i:=0;i<4096;i++ {
                    ordinal:=int64(batch*4096+i)
                    _,err=appender.Append(0,labels.FromStrings("__name__","cc_reclaim_pressure"),base+ordinal,float64(ordinal)+0.25)
                    if err!=nil {appender.Rollback();t.Fatal(err)}
                }
                if err=appender.Commit();err!=nil {t.Fatal(err)}
            }
            if testutil.ToFloat64(db.metrics.hardByteEvicted)!=1 {t.Fatal("exact first real byte prefix cut missing")}
            // Put the quiet cohort in a DISTINCT retained segment, so removing
            // the aged pressure prefix leaves real native samples to compare.
            db.hardMu.Lock()
            _,err=db.wal.NextSegmentSync()
            db.hardMu.Unlock()
            if err!=nil {t.Fatal(err)}
            // Three small committed cohorts are genuine native records, including
            // old/future SOURCE timestamps unrelated to the artificial file age.
            appender:=db.Appender(context.Background())
            for i,offset:=range []int64{0,-86400000,86400000} {
                _,err=appender.Append(0,labels.FromStrings("__name__","cc_reclaim_quiet","cohort",fmt.Sprint(i)),base+offset,float64(1701+i))
                if err!=nil {appender.Rollback();t.Fatal(err)}
            }
            if err=appender.Commit();err!=nil {t.Fatal(err)}
            phase:="quiet_empty_rotations"
            func(){
                db.hardMu.Lock();defer db.hardMu.Unlock()
                // Prevent the normal 1s maintenance goroutine from racing this
                // controlled UNIT transition. Actual production timing is intact.
                if _,err=db.wal.NextSegmentSync();err!=nil {return}
                first,_,readErr:=wlog.Segments(db.wal.Dir());if readErr!=nil {err=readErr;return}
                _,checkpoint,readErr:=wlog.LastCheckpoint(db.wal.Dir());if readErr!=nil {err=readErr;return}
                if first<=0 || checkpoint!=first-1 {err=fmt.Errorf("first byte checkpoint/prefix mismatch");return}
                for i:=0;i<120;i++ {if _,err=db.wal.NextSegmentSync();err!=nil {return}}
                var expected []record.RefSample
                expected,err=ccReclaimSamples(db.wal.Dir(),first+1);if err!=nil {return}
                if len(expected)==0 {err=fmt.Errorf("retained native sample suffix missing");return}
                if configured {
                    phase="expire_actual_send_generation"
                    err=rs.HardReclaim(func()error{return db.hard.mode.Resume(time.Now().Add(150*time.Millisecond))})
                    if err!=nil {return}
                    time.Sleep(250*time.Millisecond) // Real elapsed unit wait only.
                    if db.hard.mode.Ready() || db.hard.mode.Failed() {err=fmt.Errorf("generation expiry was not a nonsticky send fence");return}
                }
                phase="age_first_retained_closed_prefix"
                oldPath:=wlog.SegmentName(db.wal.Dir(),first)
                aged:=time.Now().Add(-116*time.Minute)
                if err=os.Chtimes(oldPath,aged,aged);err!=nil {return}
                beforeBytes:=testutil.ToFloat64(db.metrics.hardByteEvicted)
                beforeAge:=testutil.ToFloat64(db.metrics.hardAgeEvicted)
                phase="second_native_checkpoint_and_age_reclaim"
                err=db.hardMaintain(0)
                // BEFORE any fatal: preserve native error and metrics/inventory.
                t.Logf("post_age_reclaim err=%v failed=%v ready=%v bytes=%v ages=%v charged=%v unavailable=%v",
                    err,db.hard.mode.Failed(),db.hard.mode.Ready(),testutil.ToFloat64(db.metrics.hardByteEvicted),
                    testutil.ToFloat64(db.metrics.hardAgeEvicted),testutil.ToFloat64(db.metrics.hardAllocated),
                    testutil.ToFloat64(db.metrics.hardUnavailable))
                inv,inventoryErr:=db.hard.inventory(false)
                t.Logf("post_age_inventory err=%v entries=%d segments=%d charged=%d",inventoryErr,inv.entries,len(inv.segments),inv.charged)
                if err!=nil {return};if inventoryErr!=nil {err=inventoryErr;return}
                afterFirst,_,readErr:=wlog.Segments(db.wal.Dir());if readErr!=nil {err=readErr;return}
                _,afterCheckpoint,readErr:=wlog.LastCheckpoint(db.wal.Dir());if readErr!=nil {err=readErr;return}
                if afterFirst!=first+1 || afterCheckpoint!=first {err=fmt.Errorf("second native checkpoint did not advance one age prefix");return}
                if testutil.ToFloat64(db.metrics.hardAgeEvicted)!=beforeAge+1 || testutil.ToFloat64(db.metrics.hardByteEvicted)!=beforeBytes {
                    err=fmt.Errorf("age/byte causal counters conflated");return
                }
                if !db.hard.mode.Ready() || db.hard.mode.Failed() {err=fmt.Errorf("reclaimed generation unavailable");return}
                actual,readErr:=ccReclaimSamples(db.wal.Dir(),afterFirst);if readErr!=nil {err=readErr;return}
                if !reflect.DeepEqual(actual,expected) {err=fmt.Errorf("native retained timestamp/value/reference suffix changed");return}
                descriptors,readErr:=os.ReadDir("/proc/self/fd");if readErr!=nil {err=readErr;return}
                for _,entry:=range descriptors {
                    destination,linkErr:=os.Readlink(filepath.Join("/proc/self/fd",entry.Name()))
                    if linkErr==nil && (destination==oldPath || destination==oldPath+" (deleted)") {
                        err=fmt.Errorf("evicted native prefix descriptor remains open");return
                    }
                }
                if inv.charged+hardReserve>hardPhysical {err=fmt.Errorf("physical reservation exceeded")}
                phase="healthy_reclaimed_native_suffix"
            }()
            if err!=nil {t.Fatalf("causal phase=%s: %v",phase,err)}
            if !strings.Contains(phase,"healthy_reclaimed") {t.Fatal("causal transition did not complete")}
        })
    }
}
