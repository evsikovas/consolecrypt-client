// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package agent

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"golang.org/x/sys/unix"

	"github.com/prometheus/prometheus/model/metadata"
	"github.com/prometheus/prometheus/tsdb/record"
	"github.com/prometheus/prometheus/tsdb/wlog"
)

type hardSegment struct {
	index    int
	charged  int64
	modified time.Time
}
type hardInventory struct {
	charged  int64
	entries  int
	segments []hardSegment
}
type hardBudget struct {
	diskReject func(string) // Closed cause callback, bound after readonly open.
	guard     *hardGuard
	nowTest   func() time.Time // Package-private tests only; production always fixed time.Now.
	root      string
	dev       uint64
	ino       uint64
	mode      *wlog.HardMode
	high      time.Time
	rotated   time.Time
	liveLimit int64 // Production64MiB; focused tests may only reduce this.
}

func openHardBudget(root string, mode *wlog.HardMode) (*hardBudget, error) {
	if mode == nil {
		return nil, errors.New("hard mode must be bound before remote config")
	}
	guard, err := acquireHardGuard(root)
	if err != nil {
		return nil, err
	}
	h := &hardBudget{root: root, dev: guard.rootDev, ino: guard.rootIno, mode: mode, guard: guard, rotated: time.Now(), liveLimit: hardLive}
	inv, err := h.inventory(false)
	if err != nil {
		guard.Release()
		return nil, err
	}
	if inv.entries > 253 {
		guard.Release()
		return nil, wlog.ErrHardModeUnavailable
	}
	return h, nil
}

func (h *hardBudget) now() (time.Time, error) {
	n := time.Now()
	if h.nowTest != nil {
		n = h.nowTest()
	}
	if n.UnixNano() <= 0 || (!h.high.IsZero() && n.UnixNano() < h.high.UnixNano()) {
		h.mode.Fail()
		return time.Time{}, wlog.ErrHardModeUnavailable
	}
	h.high = n
	return n, nil
}

// inventory opens every object relative to its held parent with NOFOLLOW. A
// deleted-open file is not budget credit: reclamation runs only after native
// writer and EVERY remote watcher/sender are joined. No other process may open
// or mutate this dedicated root. External/uncooperative mutation is unavailable.
// Charged bytes are max(st_blocks*512, logical bytes rounded UP to64KiB), plus
// 64KiB per directory. The extra128MiB is never available for payload.
func (h *hardBudget) inventory(temporary bool) (hardInventory, error) {
	var inv hardInventory
	if err := h.guard.check(); err != nil {
		return inv, err
	}
	now, err := h.now()
	if err != nil {
		return inv, err
	}
	fd, err := unix.Open(h.root, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NOATIME, 0)
	if err != nil {
		return inv, err
	}
	root := os.NewFile(uintptr(fd), h.root)
	defer root.Close()
	var st unix.Stat_t
	if err := unix.Fstat(fd, &st); err != nil {
		return inv, err
	}
	if uint64(st.Dev) != h.dev || st.Ino != h.ino {
		return inv, wlog.ErrHardModeUnavailable
	}
	var fs unix.Statfs_t
	if err := unix.Fstatfs(fd, &fs); err != nil {
		return inv, err
	}
	if fs.Bsize <= 0 || fs.Bsize > 65536 {
		return inv, wlog.ErrHardModeUnavailable
	}
	count := 0
	var walk func(*os.File, string, int) error
	walk = func(dir *os.File, rel string, depth int) error {
		var ds unix.Stat_t
		if err := unix.Fstat(int(dir.Fd()), &ds); err != nil {
			return err
		}
		if ds.Uid != uint32(os.Getuid()) || ds.Mode&0o022 != 0 || uint64(ds.Dev) != h.dev {
			return wlog.ErrHardModeUnavailable
		}
		inv.charged += max(ds.Blocks*512, int64(65536))
		entries, err := dir.ReadDir(257)
		if err != nil && !errors.Is(err, io.EOF) {
			return err
		}
		for _, e := range entries {
			count++
			if count > 256 {
				return wlog.ErrHardModeUnavailable
			}
			name := e.Name()
			child := filepath.Join(rel, name)
			isSegment := len(name) == 8
			index, parseErr := strconv.Atoi(name)
			isSegment = isSegment && parseErr == nil && index >= 0 && fmt.Sprintf("%08d", index) == name
			isCP := strings.HasPrefix(name, "checkpoint.")
			if depth == 0 && name != "wal" && name != "lock" {
				return wlog.ErrHardModeUnavailable
			}
			if depth == 1 && !isSegment && !isCP {
				return wlog.ErrHardModeUnavailable
			}
			if depth == 2 && !isSegment {
				return wlog.ErrHardModeUnavailable
			}
			if depth > 2 {
				return wlog.ErrHardModeUnavailable
			}
			if isCP {
				part := strings.TrimPrefix(name, "checkpoint.")
				if strings.HasSuffix(part, ".tmp") {
					if !temporary {
						return wlog.ErrHardModeUnavailable
					}
					part = strings.TrimSuffix(part, ".tmp")
				}
				v, err := strconv.Atoi(part)
				if err != nil || v < 0 || fmt.Sprintf("%08d", v) != part {
					return wlog.ErrHardModeUnavailable
				}
			}
			cfd, err := unix.Openat(int(dir.Fd()), name, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
			if err != nil {
				return err
			}
			f := os.NewFile(uintptr(cfd), child)
			var cs unix.Stat_t
			err = unix.Fstat(cfd, &cs)
			if err == nil && (cs.Uid != uint32(os.Getuid()) || uint64(cs.Dev) != h.dev || cs.Mode&0o022 != 0) {
				err = wlog.ErrHardModeUnavailable
			}
			if err == nil && cs.Mode&unix.S_IFMT == unix.S_IFDIR {
				if !((depth == 0 && name == "wal") || (depth == 1 && isCP)) {
					err = wlog.ErrHardModeUnavailable
				} else {
					err = walk(f, child, depth+1)
				}
			} else if err == nil {
				if cs.Mode&unix.S_IFMT != unix.S_IFREG || cs.Nlink != 1 || cs.Size < 0 || cs.Size > hardCheckpoint {
					err = wlog.ErrHardModeUnavailable
				} else {
					charged := max(cs.Blocks*512, (cs.Size+65535)/65536*65536)
					inv.charged += charged
					if depth == 1 && isSegment {
						modified := time.Unix(cs.Mtim.Sec, cs.Mtim.Nsec)
						if modified.After(now) {
							err = wlog.ErrHardModeUnavailable
						} else {
							inv.segments = append(inv.segments, hardSegment{index, charged, modified})
						}
					}
				}
			}
			closeErr := f.Close()
			if err != nil {
				return err
			}
			if closeErr != nil {
				return closeErr
			}
		}
		return nil
	}
	if err := walk(root, "", 0); err != nil {
		return inv, err
	}
	inv.entries = count
	sort.Slice(inv.segments, func(i, j int) bool { return inv.segments[i].index < inv.segments[j].index })
	for i := 1; i < len(inv.segments); i++ {
		if inv.segments[i].index != inv.segments[i-1].index+1 || inv.segments[i].modified.Before(inv.segments[i-1].modified) {
			return inv, wlog.ErrHardModeUnavailable
		}
	}
	if inv.charged+hardReserve > hardPhysical {
		return inv, wlog.ErrHardModeUnavailable
	}
	if err := h.guard.check(); err != nil {
		return inv, err
	}
	return inv, nil
}

func (db *DB) hardMetadataBound() error {
	enc := record.Encoder{EnableSTStorage: db.opts.EnableSTStorage}
	count := 0
	for s := range db.series.allSeries() {
		count++
		if count > hardSeries || len(enc.Series([]record.RefSeries{{Ref: s.Ref(), Labels: s.Labels()}}, nil)) > 8192 {
			return wlog.ErrHardModeUnavailable
		}
		if m, ok := s.(ActiveSeriesWithMetadata); ok && !metadata.IsZeroHandle(m.Metadata()) {
			md := m.Metadata().Value()
			if len(md.Unit)+len(md.Help) > 8192 {
				return wlog.ErrHardModeUnavailable
			}
		}
	}
	// batchSize1: <=3 native records per series, each <=8KiB plus fixed
	// framing. Even charging a64KiB allocation per record stays below64MiB.
	if len(db.deleted) != 0 {
		return wlog.ErrHardModeUnavailable
	}
	return nil
}

// hardMaintain is called under the one Agent mutation gate, before ANY Log,
// rotation or checkpoint. It never deletes records via a wrapper filesystem API.
func (db *DB) hardMaintain(growth int64) error {
	h := db.hard
	if h.mode.Failed() {
		return wlog.ErrHardModeUnavailable
	}
	if h.liveLimit <= 0 || h.liveLimit > hardLive {
		return wlog.ErrHardModeUnavailable
	}
	if growth < 0 || growth > 2*hardRecord+65536 {
		return wlog.ErrHardModeUnavailable
	}
	if err := db.hardMetadataBound(); err != nil {
		return err
	}
	inv, err := h.inventory(false)
	if err != nil {
		return err
	}
	if inv.entries+3 > 256 {
		return wlog.ErrHardModeUnavailable
	}
	db.metrics.hardAllocated.Set(float64(inv.charged))
	if len(inv.segments) > 0 {
		db.metrics.hardOldestResidence.Set(max(0, time.Since(inv.segments[0].modified).Seconds()))
	}
	now, err := h.now()
	if err != nil {
		return err
	}
	_, offset, err := db.wal.LastSegmentAndOffset()
	if err != nil {
		return err
	}
	walBytes := int64(0)
	for _, s := range inv.segments {
		walBytes += s.charged
	}
	if now.Sub(h.rotated) >= hardRotation || (offset > 0 && walBytes+growth > h.liveLimit) {
		if inv.charged+growth+2*65536+hardReserve > hardPhysical {
			return wlog.ErrHardModeUnavailable
		}
		if err := h.guard.check(); err != nil {
			return err
		}
		if err := h.checkSystemHeadroom(growth + 2*65536); err != nil {
			return err
		}
		if _, err = db.wal.NextSegmentSync(); err != nil {
			return err
		}
		h.rotated = now
		inv, err = h.inventory(false)
		if err != nil {
			return err
		}
	}
	current, _, err := db.wal.LastSegmentAndOffset()
	if err != nil {
		return err
	}
	cut := -1
	live := int64(0)
	for _, s := range inv.segments {
		live += s.charged
	}
	for _, s := range inv.segments {
		if s.index >= current {
			break
		}
		if live+growth <= h.liveLimit && now.Before(s.modified.Add(hardResidence)) {
			break
		}
		cut = s.index
		live -= s.charged
	}
	if cut >= 0 {
		// Preserve the EVICTED prefix's physical deadline across all IO.
		// Last segment write can trail first ingestion by at most one batch
		// minute plus one open-segment minute, so reserve both explicitly.
		evictedDeadline := time.Time{}
		for _, seg := range inv.segments {
			if seg.index <= cut {
				deadline := seg.modified.Add(2*time.Hour - hardBatchDelay - hardRotation)
				if evictedDeadline.IsZero() || deadline.Before(evictedDeadline) {
					evictedDeadline = deadline
				}
			}
		}
		// Reserve simultaneous old/new checkpoint, not a post-write size check.
		if inv.charged+hardCheckpoint+growth+hardReserve > hardPhysical {
			return wlog.ErrHardModeUnavailable
		}
		err = db.rs.HardReclaim(func() error {
			if err := h.guard.check(); err != nil {
				return err
			}
			if err := h.checkSystemHeadroom(growth + hardCheckpoint); err != nil {
				return err
			}
			if err := Checkpoint(db.logger, db.wal, cut, 1, db.series.allSeries(), deletedSeriesIter(db.deleted, cut)); err != nil {
				return err
			}
			if err := h.guard.check(); err != nil {
				return err
			}
			db.metrics.hardLossUnknown.Set(1) // A partial failure cannot certify exact loss.
			if err := db.wal.Truncate(cut + 1); err != nil {
				return err
			}
			for _, s := range inv.segments {
				if s.index <= cut {
					db.metrics.hardEvictedSegments.Inc()
					if !now.Before(s.modified.Add(hardResidence)) {
						db.metrics.hardAgeEvicted.Inc()
					} else {
						db.metrics.hardByteEvicted.Inc()
					}
					db.metrics.hardEvictedBytes.Add(float64(s.charged))
				}
			}
			db.metrics.hardLossUnknown.Set(1)
			if err := h.guard.check(); err != nil {
				return err
			}
			if err := wlog.DeleteCheckpoints(db.wal.Dir(), cut); err != nil {
				return err
			}
			remaining, err := h.inventory(false)
			if err != nil {
				return err
			}
			if remaining.charged+growth+hardReserve > hardPhysical {
				return wlog.ErrHardModeUnavailable
			}
			postIO, err := h.now()
			if err != nil || evictedDeadline.IsZero() || !postIO.Before(evictedDeadline) {
				db.metrics.hardUnavailable.Set(1)
				h.mode.Fail()
				return wlog.ErrHardModeUnavailable
			}

			return h.mode.Resume(hardDeadline(remaining.segments, time.Now()))
		})
		if err != nil {
			return err
		}
		inv, err = h.inventory(false)
		if err != nil {
			return err
		}
	}
	if inv.charged+growth+hardReserve > hardPhysical {
		return wlog.ErrHardModeUnavailable
	}
	return nil
}


// hardSystemAvailableCause validates actual Bavail bytes separately from the
// charged-root reserve. It creates no filesystem allocation or reservation.
func hardSystemAvailableCause(blocks uint64, blockSize int64, growth int64) string {
	const maxInt64 = uint64(1<<63 - 1)
	if blockSize <= 0 || blockSize > 65536 || growth < 0 ||
		uint64(growth) > maxInt64-uint64(hardSystemHeadroom) ||
		blocks > maxInt64/uint64(blockSize) {
		return "observation_invalid"
	}
	if blocks*uint64(blockSize) < uint64(hardSystemHeadroom)+uint64(growth) {
		return "low_available"
	}
	return ""
}

func (h *hardBudget) rejectSystemDisk(cause string) error {
	if cause != "low_available" && cause != "observation_invalid" {
		cause = "observation_invalid"
	}
	if h.diskReject != nil {
		h.diskReject(cause)
	}
	h.mode.Fail()
	return wlog.ErrHardModeUnavailable
}

// Fresh observations are conditional on a cooperating filesystem. A different
// writer can consume available space after Fstatfs. This is NOT an OS reserve.
func (h *hardBudget) checkSystemHeadroom(growth int64) error {
	if h.mode.Failed() {
		return wlog.ErrHardModeUnavailable
	}
	if err := h.guard.check(); err != nil {
		return h.rejectSystemDisk("observation_invalid")
	}
	fd := int(h.guard.root.Fd())
	var st unix.Stat_t
	var fs unix.Statfs_t
	if unix.Fstat(fd, &st) != nil || uint64(st.Dev) != h.dev || st.Ino != h.ino ||
		unix.Fstatfs(fd, &fs) != nil || fs.Bavail > fs.Bfree || fs.Bfree > fs.Blocks {
		return h.rejectSystemDisk("observation_invalid")
	}
	if cause := hardSystemAvailableCause(fs.Bavail, int64(fs.Bsize), growth); cause != "" {
		return h.rejectSystemDisk(cause)
	}
	return nil
}

func hardDeadline(segments []hardSegment, now time.Time) time.Time {
	if len(segments) == 0 {
		return now.Add(hardResidence)
	}
	return segments[0].modified.Add(hardResidence)
}
