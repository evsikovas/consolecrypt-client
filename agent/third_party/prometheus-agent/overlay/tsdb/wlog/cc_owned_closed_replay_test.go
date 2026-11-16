// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package wlog

import (
    "io"
    "log/slog"
    "os"
    "path/filepath"
    "strings"
    "sync"
    "testing"
    "time"

    "github.com/prometheus/prometheus/model/labels"
    "github.com/prometheus/prometheus/tsdb/record"
    "github.com/prometheus/prometheus/util/compression"
)

// TEST-ONLY synchronization of the existing WriteTo boundary. This is NOT a
// remote-write Store implementation, successful receiver, sample codec or ACK.
type ccClosedReplayWriter struct {
    entered chan struct{}
    release chan struct{}
    once sync.Once
    mu sync.Mutex
    calls int
}
func (s *ccClosedReplayWriter) Append([]record.RefSample) bool {
    s.mu.Lock();s.calls++;s.mu.Unlock()
    if s.entered!=nil {
        s.once.Do(func(){close(s.entered)})
        <-s.release
        return false // Normal native queue shutdown also rejects a blocked Append.
    }
    return true
}
func (*ccClosedReplayWriter) AppendExemplars([]record.RefExemplar) bool {return true}
func (*ccClosedReplayWriter) AppendHistograms([]record.RefHistogramSample) bool {return true}
func (*ccClosedReplayWriter) AppendFloatHistograms([]record.RefFloatHistogramSample) bool {return true}
func (*ccClosedReplayWriter) StoreSeries([]record.RefSeries,int) {}
func (*ccClosedReplayWriter) StoreMetadata([]record.RefMetadata) {}
func (*ccClosedReplayWriter) UpdateSeriesSegment([]record.RefSeries,int) {}
func (*ccClosedReplayWriter) SeriesReset(int) {}

type ccClosedReplayLog struct {mu sync.Mutex; data []byte; overflow bool}
func (l *ccClosedReplayLog) Write(p []byte)(int,error) {
    l.mu.Lock();defer l.mu.Unlock()
    n:=len(p);left:=8192-len(l.data)
    if len(p)>left {l.overflow=true;p=p[:left]}
    l.data=append(l.data,p...)
    return n,nil
}
func (l *ccClosedReplayLog) snapshot()(string,bool) {
    l.mu.Lock();defer l.mu.Unlock()
    return string(l.data),l.overflow
}

func ccClosedReplayWAL(t *testing.T,logger *slog.Logger) (string,string) {
    t.Helper()
    root:=t.TempDir();dir:=filepath.Join(root,"wal")
    wal,err:=NewSize(logger,nil,dir,10*1024*1024,compression.None)
    if err!=nil {t.Fatal(err)}
    closed:=false
    defer func(){if !closed {wal.Close()}}()
    wal.SetSynchronousRotation()
    encoder:=record.Encoder{}
    if err=wal.Log(encoder.Series([]record.RefSeries{{Ref:1,Labels:labels.FromStrings("__name__","closed_replay")}},nil));err!=nil {t.Fatal(err)}
    // Separate native records ensure the first Append is strictly before EOF.
    for i:=0;i<4;i++ {
        if err=wal.Log(encoder.Samples([]record.RefSample{{Ref:1,T:int64(100+i),V:float64(17+i)}},nil));err!=nil {t.Fatal(err)}
    }
    if _,err=wal.NextSegmentSync();err!=nil {t.Fatal(err)}
    closeErr:=wal.Close();closed=true
    if closeErr!=nil {t.Fatal(closeErr)}
    first,last,err:=Segments(dir)
    if err!=nil || first!=0 || last!=1 {t.Fatal("native closed prefix/empty active segment missing",err)}
    return root,SegmentName(dir,0)
}

func ccClosedReplayWait(t *testing.T,done <-chan struct{},label string) {
    t.Helper()
    select {case <-done:case <-time.After(3*time.Second):t.Fatalf("bounded synchronization missing: %s",label)}
}

// Deterministic causal UNIT test. No artificial residence/OS clock, HTTP,
// receiver, fake ACK, queue deletion or AC19 duration/acceptance claim.
func TestCCOwnedHardWatcherClosedReplayCancellation(t *testing.T) {
    for _,variant:=range []string{"normal_stop","truncated_quit_open","corrupt_quit_open"} {
        t.Run(variant,func(t *testing.T) {
            logs:=&ccClosedReplayLog{}
            logger:=slog.New(slog.NewTextHandler(logs,&slog.HandlerOptions{Level:slog.LevelError}))
            defer func(){text,overflow:=logs.snapshot();t.Logf("bounded_native_error_log overflow=%v\n%s",overflow,text)}()
            root,closedPath:=ccClosedReplayWAL(t,logger)
            info,err:=os.Stat(closedPath);if err!=nil || info.Size()<16 {t.Fatal("closed native segment missing",err)}
            switch variant {
            case "truncated_quit_open":
                if err=os.Truncate(closedPath,8);err!=nil {t.Fatal(err)}
            case "corrupt_quit_open":
                raw,readErr:=os.ReadFile(closedPath);if readErr!=nil {t.Fatal(readErr)}
                if len(raw)>65536 {t.Fatal("unexpected fixture file size")}
                raw[7]^=0x40 // Corrupt the native record payload/CRC relationship.
                if err=os.WriteFile(closedPath,raw,0o600);err!=nil {t.Fatal(err)}
            }
            writer:=&ccClosedReplayWriter{}
            var releaseOnce sync.Once
            release:=func(){}
            if variant=="normal_stop" {
                writer.entered=make(chan struct{});writer.release=make(chan struct{})
                release=func(){releaseOnce.Do(func(){close(writer.release)})}
            }
            mode:=NewHardMode()
            if err=mode.Resume(time.Now().Add(time.Minute));err!=nil {t.Fatal(err)}
            watcher:=NewWatcher(NewWatcherMetrics(nil),NewLiveReaderMetrics(nil),logger,"owned-closed-replay",writer,root,false,false,false,nil)
            watcher.SetAgentHardMode(mode)
            var stopOnce sync.Once
            stopped:=make(chan struct{})
            stop:=func(){stopOnce.Do(func(){go func(){watcher.Stop();close(stopped)}()})}
            watcher.Start()
            defer func(){
                release();stop()
                ccClosedReplayWait(t,stopped,"deferred actual Watcher.Stop join")
            }()
            if variant=="normal_stop" {
                ccClosedReplayWait(t,writer.entered,"actual first closed-segment Append entered")
                // Stop closes the REAL watcher quit channel, then waits on done.
                stop()
                ccClosedReplayWait(t,watcher.quit,"actual Watcher.Stop quit closed")
                select {case <-stopped:t.Fatal("watcher joined before blocked Append released");default:}
                release()
                ccClosedReplayWait(t,stopped,"actual Watcher.Stop completed")
                writer.mu.Lock();calls:=writer.calls;writer.mu.Unlock()
                text,overflow:=logs.snapshot()
                t.Logf("normal_stop closed_bytes=%d append_calls=%d quit_closed=%v mode_failed=%v manufactured_incomplete=%v",
                    info.Size(),calls,isClosed(watcher.quit),mode.Failed(),strings.Contains(text,"incomplete retained segment"))
                if calls!=1 || overflow {t.Fatal("unbounded or wrong controlled Append observation")}
                if mode.Failed() {t.Fatal("normal joined watcher cancellation was misclassified as retained WAL corruption")}
                // Resume a genuinely new generation after the joined stop only.
                mode.Fence()
                if err=mode.Resume(time.Now().Add(time.Minute));err!=nil || !mode.Ready() {t.Fatal("normal cancellation poisoned future native generation",err)}
            } else {
                ccClosedReplayWait(t,watcher.done,"invalid closed-segment reader termination")
                // No stop signal participated in the corruption/truncation path.
                if isClosed(watcher.quit) {t.Fatal("negative test accidentally signalled quit")}
                if !mode.Failed() {t.Fatal("invalid closed WAL was accepted while quit remained open")}
                text,overflow:=logs.snapshot()
                if text=="" || overflow {t.Fatal("bounded native corruption error evidence missing")}
                stop();ccClosedReplayWait(t,stopped,"negative watcher cleanup")
            }
            // The reader defer must close the real evicted/closed segment FD.
            descriptors,readErr:=os.ReadDir("/proc/self/fd")
            if readErr!=nil {t.Fatal(readErr)}
            for _,entry:=range descriptors {
                target,linkErr:=os.Readlink(filepath.Join("/proc/self/fd",entry.Name()))
                if linkErr==nil && target==closedPath {t.Fatal("joined watcher retains native segment descriptor")}
            }
        })
    }
}

var _ io.Writer=(*ccClosedReplayLog)(nil)
var _ WriteTo=(*ccClosedReplayWriter)(nil)
