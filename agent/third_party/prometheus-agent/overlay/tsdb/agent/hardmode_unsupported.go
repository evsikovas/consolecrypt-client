// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

//go:build !linux

package agent

import (
	"github.com/prometheus/prometheus/tsdb/wlog"
	"time"
)

type hardSegment struct{}
type hardInventory struct {
	charged  int64
	entries  int
	segments []hardSegment
}
type hardBudget struct {
	diskReject func(string)
	mode  *wlog.HardMode
	guard *hardGuard
}
type hardGuard struct{}

func (*hardGuard) check() error   { return wlog.ErrHardModeUnavailable }
func (*hardGuard) Release() error { return nil }

func openHardBudget(string, *wlog.HardMode) (*hardBudget, error) {
	return nil, wlog.ErrHardModeUnavailable
}
func (*hardBudget) now() (time.Time, error) { return time.Time{}, wlog.ErrHardModeUnavailable }
func (*hardBudget) inventory(bool) (hardInventory, error) {
	return hardInventory{}, wlog.ErrHardModeUnavailable
}
func (*DB) hardMaintain(int64) error                  { return wlog.ErrHardModeUnavailable }
func (*DB) hardMetadataBound() error                  { return wlog.ErrHardModeUnavailable }
func hardDeadline([]hardSegment, time.Time) time.Time { return time.Time{} }

func (*hardBudget) checkSystemHeadroom(int64) error { return wlog.ErrHardModeUnavailable }
