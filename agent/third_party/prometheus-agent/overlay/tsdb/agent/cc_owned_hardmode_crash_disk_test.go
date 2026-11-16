// Copyright 2026 ConsoleCrypt contributors.
// Licensed under the Apache License, Version 2.0 (the Prometheus source license).

//go:build linux

package agent

// TEST ONLY. These additive tests never change the frozen production branch.
// Run only in the separately reviewed owned fixture. They prove neither
// producer-memory/power-loss durability nor the real three-hour acceptance.
import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"golang.org/x/sys/unix"
	"github.com/prometheus/common/promslog"
	"github.com/prometheus/prometheus/config"
	"github.com/prometheus/prometheus/model/labels"
	"github.com/prometheus/prometheus/tsdb/fileutil"
	"github.com/prometheus/prometheus/tsdb/record"
	"github.com/prometheus/prometheus/tsdb/wlog"
)

var ccShortDeadline time.Time // Sole, sequential opt-in suite; never production.

func ccShortOptIn(t *testing.T){
	t.Helper()
	if os.Getenv("CC_PROM_HARDMODE_SHORT_GATES")==""{t.Skip("requires explicit owned kill/8MiB ENOSPC fixture")}
	if os.Getenv("CC_PROM_HARDMODE_SHORT_GATES")!="1" || os.Getuid()!=10001 || os.Getenv("CC_PROM_HARDMODE_ENOSPC_ROOT")!="/cc-hardmode-enospc"{t.Fatal("invalid explicit owned short-gate fixture")}
	if ccShortDeadline.IsZero(){ccShortDeadline=time.Now().Add(90*time.Second)}
	ccShortAlive(t)
}

func ccShortAlive(t *testing.T){t.Helper();if !time.Now().Before(ccShortDeadline){t.Fatal("owned short-gate absolute90s deadline")}}

func ccShortSeed(t *testing.T,root string){
	t.Helper()
	if err:=os.Mkdir(root,0o700);err!=nil{t.Fatal("cannot create fresh owned queue",err)}
	f,err:=os.OpenFile(filepath.Join(root,"lock"),os.O_WRONLY|os.O_CREATE|os.O_EXCL,0o600);if err!=nil{t.Fatal(err)}
	if _,err=f.WriteString(hardLayoutMagic);err!=nil{f.Close();t.Fatal(err)}
	if err=f.Sync();err!=nil{f.Close();t.Fatal(err)}
	if err=f.Close();err!=nil{t.Fatal(err)}
	ccShortSyncDir(t,root)
}

func ccShortSyncDir(t *testing.T,path string){
	t.Helper();d,err:=fileutil.OpenDir(path);if err!=nil{t.Fatal(err)}
	err=errors.Join(d.Sync(),d.Close());if err!=nil{t.Fatal("owned directory sync failed",err)}
}

type ccShortLog struct{mu sync.Mutex;data []byte;overflow bool}
func(b *ccShortLog)Write(p []byte)(int,error){
	b.mu.Lock();defer b.mu.Unlock();n:=len(p)
	if len(b.data)+n>65536{b.overflow=true;left:=65536-len(b.data);if left>0{b.data=append(b.data,p[:left]...)};return n,nil}
	b.data=append(b.data,p...);return n,nil
}
func(b *ccShortLog)Observation()([]byte,bool){b.mu.Lock();defer b.mu.Unlock();return append([]byte(nil),b.data...),b.overflow}

type ccShortReady struct{Format string `json:"format"`;Base int64 `json:"base"`;Samples int `json:"samples"`;Synced bool `json:"synced"`}
type ccShortPair struct{cohort string;ts int64;value float64}

func ccShortPairs(base int64)[]ccShortPair{
	out:=make([]ccShortPair,0,200)
	for i:=0;i<100;i++{out=append(out,ccShortPair{"old",base-30*60*1000+int64(i)*1000,float64(i)+0.25})}
	for i:=0;i<100;i++{out=append(out,ccShortPair{"future",base+24*60*60*1000+int64(i)*1000,-float64(i)-0.75})}
	return out
}

func ccShortKillChild(t *testing.T){
	t.Helper();root,ready:=os.Getenv("CC_SHORT_QUEUE"),os.Getenv("CC_SHORT_READY")
	base,err:=strconv.ParseInt(os.Getenv("CC_SHORT_BASE"),10,64)
	if err!=nil || base<=0 || !filepath.IsAbs(root) || filepath.Clean(root)!=root || filepath.Base(root)!="queue" || filepath.Dir(root)!=filepath.Dir(ready) || filepath.Base(ready)!="ready.json"{t.Fatal("invalid closed child fixture")}
	db,r,err:=hardTestOpen(t,root);if err!=nil{r.Close();t.Fatal(err)}
	// Deliberately no deferred graceful close: the parent MUST kill this live
	// process and verify SIGKILL, rather than accept child normal shutdown.
	a:=db.Appender(context.Background())
	for _,p:=range ccShortPairs(base){
		if _,err=a.Append(0,labels.FromStrings("__name__","cc_short_durable","cohort",p.cohort),p.ts,p.value);err!=nil{t.Fatal(err)}
	}
	if err=a.Commit();err!=nil{t.Fatal(err)}
	if err=db.wal.Sync();err!=nil{t.Fatal("native seed Sync failed",err)}
	ccShortSyncDir(t,db.wal.Dir());ccShortSyncDir(t,root)
	raw,err:=json.Marshal(ccShortReady{"cc-hardmode-owned-ready/v1",base,200,true});if err!=nil{t.Fatal(err)}
	f,err:=os.OpenFile(ready+".pending",os.O_WRONLY|os.O_CREATE|os.O_EXCL,0o600);if err!=nil{t.Fatal(err)}
	if _,err=f.Write(raw);err!=nil{f.Close();t.Fatal(err)}
	if err=errors.Join(f.Sync(),f.Close());err!=nil{t.Fatal(err)}
	if err=os.Rename(ready+".pending",ready);err!=nil{t.Fatal(err)}
	for end:=time.Now().Add(40*time.Second);time.Now().Before(end);{time.Sleep(100*time.Millisecond)}
	t.Fatal("child was not SIGKILLed by its owning parent")
}

func ccShortReadReady(path string,base int64)(bool,error){
	fd,err:=unix.Open(path,unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NOATIME,0)
	if errors.Is(err,unix.ENOENT){return false,nil};if err!=nil{return false,err}
	f:=os.NewFile(uintptr(fd),"owned ready");defer f.Close();var st unix.Stat_t
	if err=unix.Fstat(fd,&st);err!=nil{return false,err}
	if st.Mode&unix.S_IFMT!=unix.S_IFREG || st.Mode&0o777!=0o600 || st.Uid!=10001 || st.Nlink!=1 || st.Size<=0 || st.Size>1024{return false,errors.New("invalid owned ready metadata")}
	raw,err:=io.ReadAll(io.LimitReader(f,1025));if err!=nil || len(raw)>1024{return false,errors.New("ready read budget")}
	var frame ccShortReady;d:=json.NewDecoder(bytes.NewReader(raw));d.DisallowUnknownFields()
	if err=d.Decode(&frame);err!=nil{return false,err};var extra any;if d.Decode(&extra)!=io.EOF{return false,errors.New("extra ready frame")}
	canonical,err:=json.Marshal(frame)
	if err!=nil || !bytes.Equal(canonical,raw) || frame.Format!="cc-hardmode-owned-ready/v1" || frame.Base!=base || frame.Samples!=200 || !frame.Synced{return false,errors.New("wrong native ready observation")}
	return true,nil
}

func ccShortPairKey(p ccShortPair)string{return fmt.Sprintf("%s/%d/%016x",p.cohort,p.ts,math.Float64bits(p.value))}

func ccShortExactNativePairs(t *testing.T,root string,base int64){
	t.Helper();expected:=map[string]bool{};for _,p:=range ccShortPairs(base){expected[ccShortPairKey(p)]=false}
	sr,err:=wlog.NewSegmentsReader(filepath.Join(root,"wal"));if err!=nil{t.Fatal(err)};defer sr.Close()
	r:=wlog.NewReader(sr);dec:=record.NewDecoder(labels.NewSymbolTable(),promslog.NewNopLogger());cohorts:=map[uint64]string{};count:=0
	for r.Next(){
		raw:=r.Record()
		switch dec.Type(raw){
		case record.Series:
			series,err:=dec.Series(raw,nil);if err!=nil{t.Fatal(err)}
			for _,s:=range series{if s.Labels.Get("__name__")!="cc_short_durable"{t.Fatal("unexpected durable series")};cohorts[uint64(s.Ref)]=s.Labels.Get("cohort")}
		case record.Samples,record.SamplesV2:
			samples,err:=dec.Samples(raw,nil);if err!=nil{t.Fatal(err)}
			for _,s:=range samples{key:=ccShortPairKey(ccShortPair{cohorts[uint64(s.Ref)],s.T,s.V});seen,ok:=expected[key];if !ok || seen{t.Fatal("missing/duplicate/rewritten original durable pair")};expected[key]=true;count++}
		default:t.Fatal("unexpected native record in fixed scalar seed")
		}
	}
	if err=r.Err();err!=nil{t.Fatal(err)};if count!=200{t.Fatal("not all200 original durable pairs recovered")}
	for _,seen:=range expected{if !seen{t.Fatal("original durable pair absent")}}
}

func TestCCOwnedHardmodeDurableKillReplay(t *testing.T){
	ccShortOptIn(t)
	if role:=os.Getenv("CC_SHORT_CHILD");role!=""{if role!="1"{t.Fatal("invalid child role")};ccShortKillChild(t);return}
	fixture:=t.TempDir();queue,ready:=filepath.Join(fixture,"queue"),filepath.Join(fixture,"ready.json");ccShortSeed(t,queue)
	base:=time.Now().UnixMilli();exe,err:=os.Executable();if err!=nil{t.Fatal(err)}
	ctx,cancel:=context.WithTimeout(context.Background(),30*time.Second);defer cancel()
	cmd:=exec.CommandContext(ctx,exe,"-test.run=^TestCCOwnedHardmodeDurableKillReplay$")
	cmd.Env=append(os.Environ(),"CC_SHORT_CHILD=1","CC_SHORT_QUEUE="+queue,"CC_SHORT_READY="+ready,"CC_SHORT_BASE="+strconv.FormatInt(base,10))
	log:=&ccShortLog{};cmd.Stdout=log;cmd.Stderr=log
	if err=cmd.Start();err!=nil{t.Fatal(err)}
	wait:=make(chan error,1);go func(){wait<-cmd.Wait()}();reaped:=false
	defer func(){if !reaped{cmd.Process.Kill();select{case <-wait:case <-time.After(5*time.Second):t.Error("own killed child not reaped")}}}()
	for end:=time.Now().Add(15*time.Second);;{
		ccShortAlive(t);if !time.Now().Before(end){t.Fatal("child native Sync readiness deadline")}
		select{case err:=<-wait:reaped=true;raw,_:=log.Observation();t.Fatalf("child exited beforeSync ready: %v %s",err,raw);default:}
		ok,err:=ccShortReadReady(ready,base);if err!=nil{t.Fatal(err)};if ok{break};time.Sleep(20*time.Millisecond)
	}
	if ctx.Err()!=nil{t.Fatal("child command expired before deliberate kill")}
	if err=cmd.Process.Kill();err!=nil{t.Fatal("exact own SIGKILL failed",err)}
	select{case err=<-wait:reaped=true;case <-time.After(5*time.Second):t.Fatal("own SIGKILL reap deadline")}
	if cmd.ProcessState==nil{t.Fatal("owned child exit observation absent")}
	status,ok:=cmd.ProcessState.Sys().(syscall.WaitStatus)
	if err==nil || !ok || !status.Signaled() || status.Signal()!=syscall.SIGKILL || ctx.Err()!=nil{t.Fatal("not an explicit owned SIGKILL outcome")}
	if _,overflow:=log.Observation();overflow{t.Fatal("bounded child log overflow")}
	before:=hardObserveWholeRoot(t,queue)
	db,r,err:=hardTestOpen(t,queue);if err!=nil{r.Close();t.Fatal("native crash replay failed",err)}
	defer r.Close();defer db.Close()
	ccShortExactNativePairs(t,queue,base)
	after:=hardObserveWholeRoot(t,queue)
	for path,original:=range before{if strings.HasPrefix(path,"wal/") && !reflect.DeepEqual(original,after[path]){t.Fatal("native recovery rewrote original synced WAL")}}
	if !reflect.DeepEqual(before["lock"],after["lock"]){t.Fatal("native crash recovery replaced original guard")}
	t.Log("ownedSIGKILL+reap; all200 original past/future Sync-seeded pairs recovered; no memory/powerloss/ACK claim")
}

func ccShortPrivateEnospcRoot(t *testing.T)string{
	t.Helper();root:=os.Getenv("CC_PROM_HARDMODE_ENOSPC_ROOT")
	if root!="/cc-hardmode-enospc" || strings.HasPrefix(filepath.Clean(os.TempDir()),root){t.Fatal("invalid private ENOSPC topology")}
	fd,err:=unix.Open(root,unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC,0);if err!=nil{t.Fatal(err)};defer unix.Close(fd)
	var st unix.Stat_t;var fs unix.Statfs_t
	if unix.Fstat(fd,&st)!=nil || unix.Fstatfs(fd,&fs)!=nil || st.Uid!=10001 || st.Mode&0o777!=0o700 || fs.Type!=unix.TMPFS_MAGIC || fs.Bsize!=4096 || fs.Blocks!=2048{t.Fatal("not exact owned8MiB private tmpfs")}
	entries,err:=os.ReadDir(root);if err!=nil || len(entries)!=0{t.Fatal("private ENOSPC mount not fresh/empty")};return root
}

func TestCCOwnedHardmodeNativeEnospc(t *testing.T){
	ccShortOptIn(t);root:=ccShortPrivateEnospcRoot(t);queue:=filepath.Join(root,"queue");ccShortSeed(t,queue)
	var requests atomic.Int32
	server:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){requests.Add(1);w.WriteHeader(http.StatusNoContent)}));defer server.Close()
	db,rs,err:=hardTestOpen(t,queue);if err!=nil{rs.Close();t.Fatal(err)}
	closed:=false;defer func(){if !closed{db.hard.mode.Fail();db.Close();rs.Close()}}()
	conf,err:=config.Load(fmt.Sprintf(`remote_write:
- name: cc_short_enospc
  url: %s/api/v1/write
  remote_timeout: 1s
  follow_redirects: false
  proxy_from_environment: false
  enable_http2: false
  send_exemplars: false
  send_native_histograms: false
  metadata_config:
    send: false
  queue_config:
    capacity: 128
    min_shards: 1
    max_shards: 1
    max_samples_per_send: 32
    batch_send_deadline: 1s
    min_backoff: 100ms
    max_backoff: 1s
    sample_age_limit: 0s
`,server.URL),promslog.NewNopLogger());if err!=nil{t.Fatal(err)}
	if err=rs.ApplyConfig(conf);err!=nil{t.Fatal(err)};db.SetWriteNotified(rs)
	filler,err:=os.OpenFile(filepath.Join(root,"owned-filler"),os.O_WRONLY|os.O_CREATE|os.O_EXCL,0o600);if err!=nil{t.Fatal(err)};defer filler.Close()
	chunk:=bytes.Repeat([]byte{0x5a},65536);written:=0
	for written<8*1024*1024{ccShortAlive(t);n,e:=filler.Write(chunk);written+=n;if e!=nil{if !errors.Is(e,unix.ENOSPC) && !errors.Is(e,io.ErrShortWrite){t.Fatal("unexpected private filler failure",e)};break}}
	if n,e:=filler.Write([]byte{0x5a});n!=0 || !errors.Is(e,unix.ENOSPC){t.Fatal("private filler did not reach REAL ENOSPC")}
	st,err:=filler.Stat();if err!=nil || st.Size()<4096 || st.Size()>8*1024*1024{t.Fatal("invalid bounded filler")}
	if err=filler.Truncate(st.Size()-4096);err!=nil{t.Fatal(err)}
	var fs unix.Statfs_t;if unix.Statfs(root,&fs)!=nil || fs.Bfree!=1{t.Fatal("fixture did not release exact one4KiB page")}
	a:=db.Appender(context.Background()).(*appender)
	if _,err=a.Append(0,labels.FromStrings("__name__","cc_short_enospc","payload",strings.Repeat("p",7600)),1,8.25);err!=nil{t.Fatal(err)}
	enc:=record.Encoder{};encoded:=len(enc.Series(a.pendingSeries,nil));if encoded<=4096 || encoded>8192{t.Fatal("invalid native partial-write quantum")}
	err=a.Commit();kind:=""
	if errors.Is(err,unix.ENOSPC){kind="native_enospc"}else if errors.Is(err,io.ErrShortWrite){kind="native_partial_short_write"}else{t.Fatal("native Log did not fail from disk exhaustion",err)}
	if n,e:=filler.WriteAt([]byte{0x5a},st.Size()-4096);n!=0 || !errors.Is(e,unix.ENOSPC){t.Fatal("native partialLog not followed by REAL filesystem ENOSPC")}
	partial,err:=os.ReadFile(wlog.SegmentName(filepath.Join(queue,"wal"),0));if err!=nil || len(partial)==0 || len(partial)>=encoded{t.Fatal("native failure did not leave actual incomplete nonempty record")}
	if !db.hard.mode.Failed(){t.Fatal("native disk failure not sticky")}
	if _,release,e:=rs.AgentHardMode().BeginSend(context.Background());e==nil{release();t.Fatal("failed native generation admitted a send")}
	before:=hardObserveWholeRoot(t,queue)
	closeErr:=db.Close();rsErr:=rs.Close();closed=true
	if closeErr==nil || rsErr!=nil{t.Fatal("unexpected unavailable pipeline close outcome")}
	if requests.Load()!=0{t.Fatal("failed fresh batch reached actual HTTP client")}
	if !reflect.DeepEqual(before,hardObserveWholeRoot(t,queue)){t.Fatal("unavailable close padded/repaired/mutated queue")}
	if err=filler.Truncate(0);err!=nil{t.Fatal(err)} // ONLYowned non-queue filler, AFTER pipeline joined.
	recoveryBefore:=hardObserveWholeRoot(t,queue)
	reopened,r2,err:=hardTestOpen(t,queue);if reopened!=nil{reopened.Close()};r2.Close()
	if err==nil || reopened!=nil{t.Fatal("incomplete native record was silently repaired/adopted")}
	if !reflect.DeepEqual(recoveryBefore,hardObserveWholeRoot(t,queue)){t.Fatal("corrupt replay mutated original queue or guard")}
	ccShortAlive(t);t.Logf("REAL private8MiB ENOSPC; %s; partialrecord/noHTTP/noPadding/noRepair; not physical hardware/3h proof",kind)
}
