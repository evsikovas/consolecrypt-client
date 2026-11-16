// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package agent

import (
	"context"
	"crypto/sha256"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/prometheus/client_golang/prometheus/testutil"
	"github.com/prometheus/prometheus/model/labels"
	"github.com/prometheus/prometheus/tsdb/fileutil"
	"github.com/prometheus/prometheus/tsdb/wlog"
	"golang.org/x/sys/unix"
)

type hardRootObservation struct {
	Dev      uint64
	Ino      uint64
	Mode     uint32
	Uid      uint32
	Gid      uint32
	Nlink    uint64
	Size     int64
	Blocks   int64
	Mtime    unix.Timespec
	Ctime    unix.Timespec
	Contents [32]byte
	Entries  []string
}

// Deliberately includes the root, directories AND lock, not just WAL payload.
// Access time is excluded: ordinary native read-only replay may update atime.
func hardObserveWholeRoot(t *testing.T, root string) map[string]hardRootObservation {
	t.Helper()
	out := map[string]hardRootObservation{}
	err := filepath.WalkDir(root, func(path string, e os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		var st unix.Stat_t
		if err = unix.Lstat(path, &st); err != nil {
			return err
		}
		v := hardRootObservation{Dev: uint64(st.Dev), Ino: st.Ino, Mode: st.Mode, Uid: st.Uid, Gid: st.Gid, Nlink: st.Nlink, Size: st.Size, Blocks: st.Blocks, Mtime: st.Mtim, Ctime: st.Ctim}
		if e.IsDir() {
			entries, err := os.ReadDir(path)
			if err != nil {
				return err
			}
			for _, x := range entries {
				v.Entries = append(v.Entries, x.Name())
			}
		} else {
			raw, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			v.Contents = sha256.Sum256(raw)
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		out[rel] = v
		return nil
	})
	if err != nil {
		t.Fatal("cannot observe owned whole-root fixture", err)
	}
	return out
}

func TestHardModeFailedOpenPreservesWholeRoot(t *testing.T) {
	for _, kind := range []string{"corrupt-existing-guard", "corrupt-missing-guard", "legacy-empty-guard", "wrong-marker", "fresh-missing-guard"} {
		t.Run(kind, func(t *testing.T) {
			root := t.TempDir()
			if kind == "corrupt-existing-guard" || kind == "legacy-empty-guard" || kind == "wrong-marker" {
				magic := hardLayoutMagic
				if kind == "legacy-empty-guard" {
					magic = ""
				}
				if kind == "wrong-marker" {
					magic = "foreign layout\n"
				}
				if err := os.WriteFile(filepath.Join(root, "lock"), []byte(magic), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			if strings.HasPrefix(kind, "corrupt-") {
				wal := filepath.Join(root, "wal")
				if err := os.Mkdir(wal, 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(filepath.Join(wal, "00000000"), []byte("invalid native WAL frame"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			before := hardObserveWholeRoot(t, root)
			db, r, err := hardTestOpen(t, root)
			if db != nil {
				db.Close()
			}
			r.Close()
			if err == nil {
				t.Fatal("incompatible/corrupt/missing guard was adopted")
			}
			after := hardObserveWholeRoot(t, root)
			if !reflect.DeepEqual(before, after) {
				t.Fatal("failed Open mutated whole-root entries/inodes/contents/mode/mtime/ctime")
			}
		})
	}
}

func TestHardModeExistingGuardFlockAndNoUnlink(t *testing.T) {
	if root := os.Getenv("CC_HARDMODE_GUARD_CHILD"); root != "" {
		if !filepath.IsAbs(root) || !strings.HasPrefix(filepath.Clean(root), filepath.Clean(os.TempDir())+string(os.PathSeparator)) {
			t.Fatal("invalid owned child root")
		}
		r, existed, err := fileutil.Flock(filepath.Join(root, "lock"))
		if os.Getenv("CC_HARDMODE_GUARD_EXPECT") == "blocked" {
			if err == nil {
				r.Release()
				t.Fatal("native writer entered held hardmode guard")
			}
			if !existed {
				t.Fatal("native guard unexpectedly absent")
			}
			return
		}
		if err != nil || !existed {
			t.Fatal("native compatible guard unavailable after release")
		}
		if err = r.Release(); err != nil {
			t.Fatal(err)
		}
		return
	}
	root := hardTestOwnedRoot(t)
	before := hardObserveWholeRoot(t, root)
	h, err := openHardBudget(root, wlog.NewHardMode())
	if err != nil {
		t.Fatal(err)
	}
	defer h.guard.Release()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	child := func(expect string) {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, exe, "-test.run=^TestHardModeExistingGuardFlockAndNoUnlink$")
		cmd.Env = append(os.Environ(), "CC_HARDMODE_GUARD_CHILD="+root, "CC_HARDMODE_GUARD_EXPECT="+expect)
		out, err := cmd.CombinedOutput()
		if err != nil || len(out) > 65536 {
			t.Fatal("owned native flock child failed", err)
		}
	}
	child("blocked")
	if err = h.guard.Release(); err != nil {
		t.Fatal(err)
	}
	child("free")
	if !reflect.DeepEqual(before, hardObserveWholeRoot(t, root)) {
		t.Fatal("flock/release changed or unlinked original guard")
	}
}

func TestHardModeLostGuardCloseDoesNotFlushTail(t *testing.T) {
	for _, lost := range []bool{false, true} {
		t.Run(fmt.Sprint(lost), func(t *testing.T) {
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
				}
				r.Close()
			}()
			a := db.Appender(context.Background())
			if _, err = a.Append(0, labels.FromStrings("__name__", "guard_tail"), 1, 9); err != nil {
				t.Fatal(err)
			}
			if err = a.Commit(); err != nil {
				t.Fatal(err)
			}
			p := wlog.SegmentName(db.wal.Dir(), 0)
			before, err := os.ReadFile(p)
			if err != nil {
				t.Fatal(err)
			}
			if len(before) == 0 || len(before)%32768 == 0 {
				t.Fatal("fixture must exercise native partial tail padding")
			}
			lock, parked := filepath.Join(root, "lock"), filepath.Join(root, "owned-parked-lock")
			if lost {
				if err = os.Rename(lock, parked); err != nil {
					t.Fatal(err)
				}
			} else {
				// Direct sticky-state injection, not a claim of real disk failure.
				db.hard.mode.Fail()
			}
			err = db.Close()
			closed = true
			if err == nil {
				t.Fatal("unavailable close reported healthy")
			}
			after, err := os.ReadFile(p)
			if err != nil || string(after) != string(before) {
				t.Fatal("unavailable cleanup flushed/padded/mutated WAL tail")
			}
			if lost {
				if _, err = os.Lstat(lock); !os.IsNotExist(err) {
					t.Fatal("guard recreated during close")
				}
				// Same original inode restoration only as owned fixture cleanup, after close.
				if err = os.Rename(parked, lock); err != nil {
					t.Fatal(err)
				}
			} else {
				raw, err := os.ReadFile(lock)
				if err != nil || string(raw) != hardLayoutMagic {
					t.Fatal("valid guard changed during failed close")
				}
			}
		})
	}
}

func TestHardModePostIoChecksEvictedPhysicalDeadline(t *testing.T) {
	root := hardTestOwnedRoot(t)
	db, r, err := hardTestOpen(t, root)
	if err != nil {
		r.Close()
		t.Fatal(err)
	}
	defer r.Close()
	defer db.Close()
	a := db.Appender(context.Background())
	if _, err = a.Append(0, labels.FromStrings("__name__", "slow_reclaim"), 1, 4); err != nil {
		t.Fatal(err)
	}
	if err = a.Commit(); err != nil {
		t.Fatal(err)
	}
	db.hardMu.Lock()
	defer db.hardMu.Unlock()
	if _, err = db.wal.NextSegmentSync(); err != nil {
		t.Fatal(err)
	}
	oldPath := wlog.SegmentName(db.wal.Dir(), 0)
	base := time.Now()
	old := base.Add(-116 * time.Minute)
	if err = os.Chtimes(oldPath, old, old); err != nil {
		t.Fatal(err)
	}
	db.hard.nowTest = func() time.Time {
		if _, err := os.Lstat(oldPath); os.IsNotExist(err) {
			return base.Add(4 * time.Minute)
		}
		return base
	}
	err = db.hardMaintain(0)
	if err == nil || !db.hard.mode.Failed() || testutil.ToFloat64(db.metrics.hardUnavailable) != 1 {
		t.Fatal("known late IO resumed healthy from fresh survivor")
	}
	if _, err = os.Lstat(oldPath); !os.IsNotExist(err) {
		t.Fatal("clock fault was not injected after actual native deletion")
	}
	if _, idx, err := wlog.LastCheckpoint(db.wal.Dir()); err != nil || idx != 0 {
		t.Fatal("native checkpoint not committed before late observation", fmt.Sprint(err))
	}
}
