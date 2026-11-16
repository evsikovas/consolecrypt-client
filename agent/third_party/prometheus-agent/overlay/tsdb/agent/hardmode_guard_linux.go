// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

//go:build linux

package agent

import (
	"errors"
	"os"
	"sync"

	"github.com/prometheus/prometheus/tsdb/wlog"
	"golang.org/x/sys/unix"
)

// This fixed PUBLIC layout marker is not an arrival/ACK record or authority.
// An operator provisions it only in a fresh dedicated root. Open never creates,
// repairs, writes or unlinks it. Native Flock ignores content and uses the SAME
// Linux flock primitive, so a live legacy writer still excludes this writer.
const hardLayoutMagic = "cc-prom-agent-hardmode/v1\n"

type hardGuard struct {
	mu       sync.Mutex
	root     *os.File
	file     *os.File
	dev      uint64
	ino      uint64
	rootDev  uint64
	rootIno  uint64
	released bool
}

func acquireHardGuard(root string) (*hardGuard, error) {
	rfd, err := unix.Open(root, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC|unix.O_NOATIME, 0)
	if err != nil {
		return nil, err
	}
	r := os.NewFile(uintptr(rfd), root)
	var rs unix.Stat_t
	if err = unix.Fstat(rfd, &rs); err != nil {
		r.Close()
		return nil, err
	}
	if rs.Uid != uint32(os.Getuid()) || rs.Mode&0o022 != 0 {
		r.Close()
		return nil, wlog.ErrHardModeUnavailable
	}
	fd, err := unix.Openat(rfd, "lock", unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC|unix.O_NOATIME, 0)
	if err != nil {
		r.Close()
		return nil, err
	}
	f := os.NewFile(uintptr(fd), "existing hardmode lock")
	var s unix.Stat_t
	if err = unix.Fstat(fd, &s); err != nil {
		f.Close()
		r.Close()
		return nil, err
	}
	g := &hardGuard{root: r, file: f, dev: uint64(s.Dev), ino: s.Ino, rootDev: uint64(rs.Dev), rootIno: rs.Ino}
	if err = unix.Flock(fd, unix.LOCK_EX|unix.LOCK_NB); err != nil {
		f.Close()
		r.Close()
		return nil, err
	}
	if err = g.check(); err != nil {
		g.Release()
		return nil, err
	}
	return g, nil
}

func (g *hardGuard) check() error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.released {
		return wlog.ErrHardModeUnavailable
	}
	var held, path, root unix.Stat_t
	if err := unix.Fstat(int(g.root.Fd()), &root); err != nil {
		return err
	}
	if uint64(root.Dev) != g.rootDev || root.Ino != g.rootIno || root.Uid != uint32(os.Getuid()) || root.Mode&0o022 != 0 {
		return wlog.ErrHardModeUnavailable
	}
	if err := unix.Fstat(int(g.file.Fd()), &held); err != nil {
		return err
	}
	if err := unix.Fstatat(int(g.root.Fd()), "lock", &path, unix.AT_SYMLINK_NOFOLLOW); err != nil {
		return err
	}
	for _, s := range []unix.Stat_t{held, path} {
		if uint64(s.Dev) != g.dev || s.Ino != g.ino || s.Mode&unix.S_IFMT != unix.S_IFREG || s.Uid != uint32(os.Getuid()) || s.Mode&0o022 != 0 || s.Nlink != 1 || s.Size != int64(len(hardLayoutMagic)) {
			return wlog.ErrHardModeUnavailable
		}
	}
	buf := make([]byte, len(hardLayoutMagic)+1)
	n, err := unix.Pread(int(g.file.Fd()), buf, 0)
	if err != nil {
		return err
	}
	if n != len(hardLayoutMagic) || string(buf[:n]) != hardLayoutMagic {
		return wlog.ErrHardModeUnavailable
	}
	return nil
}

// Release changes only process-owned locks/FDs. In particular it NEVER calls
// DirLocker.Release, which unlinks the shared native lock pathname.
func (g *hardGuard) Release() error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.released {
		return nil
	}
	g.released = true
	return errors.Join(unix.Flock(int(g.file.Fd()), unix.LOCK_UN|unix.LOCK_NB), g.file.Close(), g.root.Close())
}
