// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

package remote

import (
	"bytes"
	"context"
	"errors"
	"io"
	"log/slog"
	"math"
	"net/http"
	"net/http/httptest"
	"net/url"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/golang/snappy"
	remoteapi "github.com/prometheus/client_golang/exp/api/remote"
	commoncfg "github.com/prometheus/common/config"
	"github.com/prometheus/common/model"

	"github.com/prometheus/prometheus/config"
	"github.com/prometheus/prometheus/prompb"
	"github.com/prometheus/prometheus/tsdb/wlog"
)

func TestHardModeJitterBounds(t *testing.T) {
	for _, tc := range []struct {
		name string
		base, after, low, high time.Duration
	}{
		{"one_nanosecond", 1, 0, 1, 1},
		{"odd_three_nanoseconds", 3, 0, 2, 3},
		{"minimum_profile_base", 30 * time.Millisecond, 0, 15 * time.Millisecond, 30 * time.Millisecond},
		{"maximum_base", time.Second, 0, time.Second / 2, time.Second},
		{"past_header", time.Second, -time.Second, time.Second / 2, time.Second},
		{"positive_header", time.Second, 3 * time.Second, 3 * time.Second, 3500 * time.Millisecond},
		{"large_header", time.Second, 24 * time.Hour, 24 * time.Hour, 24*time.Hour + time.Second/2},
		{"largest_safe_header", time.Second, time.Duration(math.MaxInt64) - time.Second/2, time.Duration(math.MaxInt64) - time.Second/2, time.Duration(math.MaxInt64)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			low, span, err := hardRetryBounds(tc.base, tc.after)
			if err != nil || low != tc.low || low+span != tc.high {
				t.Fatalf("bounds: low=%v span=%v error=%v", low, span, err)
			}
			for _, draw := range []uint64{0, uint64(span) / 2, uint64(span)} {
				got, err := hardRetryDelay(tc.base, tc.after, draw)
				if err != nil || got != tc.low+time.Duration(draw) || got < tc.low || got > tc.high {
					t.Fatalf("draw=%d got=%v error=%v", draw, got, err)
				}
			}
			if _, err := hardRetryDelay(tc.base, tc.after, uint64(span)+1); err == nil {
				t.Fatal("out of range draw accepted")
			}
		})
	}
	for _, tc := range [][2]time.Duration{{0, 0}, {-1, 0}, {time.Second + 1, 0}, {time.Second, time.Duration(math.MaxInt64)}} {
		if _, _, err := hardRetryBounds(tc[0], tc[1]); err == nil {
			t.Fatal("invalid bounds or overflow accepted")
		}
	}
}

func TestHardModeJitterIndependentBase(t *testing.T) {
	base := 30 * time.Millisecond
	want := []time.Duration{60 * time.Millisecond, 120 * time.Millisecond, 240 * time.Millisecond, 480 * time.Millisecond, 960 * time.Millisecond, time.Second, time.Second}
	for _, next := range want {
		// Header and random selection never become the next exponential base.
		if _, err := hardRetryDelay(base, time.Hour, 0); err != nil {
			t.Fatal(err)
		}
		base = hardNextRetryBase(base, time.Second)
		if base != next {
			t.Fatalf("base=%v want=%v", base, next)
		}
	}
	if got := hardNextRetryBase(1, 1); got != 1 {
		t.Fatal("minimum cap overflowed")
	}
}

func jitterQueue(t *testing.T) *QueueManager {
	t.Helper()
	h := wlog.NewHardMode()
	if err := h.Resume(time.Now().Add(time.Minute)); err != nil {
		t.Fatal(err)
	}
	return &QueueManager{hard: h, cfg: config.QueueConfig{MinBackoff: model.Duration(30 * time.Millisecond), MaxBackoff: model.Duration(time.Second)}, logger: slog.New(slog.NewTextHandler(io.Discard, nil))}
}

func TestHardModeJitterCancellationAndGeneration(t *testing.T) {
	for _, mode := range []string{"context", "generation", "deadline", "generation_deadline"} {
		t.Run(mode, func(t *testing.T) {
			q := jitterQueue(t)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			if mode == "deadline" {
				var deadlineCancel context.CancelFunc
				ctx, deadlineCancel = context.WithTimeout(ctx, 50*time.Millisecond)
				defer deadlineCancel()
			}
			if mode == "generation_deadline" {
				// The sender parent has no deadline: only the original native
				// generation deadline must stop this hour-long Retry-After.
				ctx = context.Background()
				q.hard.Fence()
				if err := q.hard.Resume(time.Now().Add(50 * time.Millisecond)); err != nil {
					t.Fatal(err)
				}
			}
			entered := make(chan struct{})
			result := make(chan error, 1)
			attempts, retries := 0, 0
			go func() {
				result <- q.sendWriteRequestWithBackoff(ctx, func(int) error {
					attempts++
					if attempts == 1 {
						close(entered)
					}
					return RecoverableError{error: errors.New("retry fixture"), retryAfter: model.Duration(time.Hour)}
				}, func() { retries++ })
			}()
			select {
			case <-entered:
			case err := <-result:
				t.Fatalf("sender ended before first attempt: %v", err)
			case <-time.After(time.Second):
				t.Fatal("first attempt not entered")
			}
			switch mode {
			case "context":
				cancel()
			case "generation":
				// Resume before the waiter is scheduled must not revive its old
				// generation. Fence does not wait for a counterfeit Store count.
				q.hard.Fence()
				if err := q.hard.Resume(time.Now().Add(time.Minute)); err != nil {
					t.Fatal(err)
				}
			}
			select {
			case err := <-result:
				if err == nil || attempts != 1 || retries != 0 {
					t.Fatalf("error=%v attempts=%d retries=%d", err, attempts, retries)
				}
			case <-time.After(time.Second):
				t.Fatal("original cancellation did not abort Retry-After wait")
			}
		})
	}
}

func TestHardModeJitterAlreadyCancelled(t *testing.T) {
	q := jitterQueue(t)
	generation, err := q.hard.RetryDone()
	if err != nil {
		t.Fatal(err)
	}
	q.hard.Fence()
	if err := q.hard.Resume(time.Now().Add(time.Minute)); err != nil {
		t.Fatal(err)
	}
	// Even a simultaneously ready zero-delay timer cannot defeat the old
	// generation. The accessor neither consumed a Store nor renewed it.
	if err := q.waitHardRetry(context.Background(), generation, 0); err == nil {
		t.Fatal("cancelled generation revived by Resume/timer")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	attempts, retries := 0, 0
	err = q.sendWriteRequestWithBackoff(ctx, func(int) error { attempts++; return nil }, func() { retries++ })
	if !errors.Is(err, context.Canceled) || attempts != 0 || retries != 0 {
		t.Fatalf("error=%v attempts=%d retries=%d", err, attempts, retries)
	}
}

func TestHardModeJitterCancelBeforeNextAttempt(t *testing.T) {
	q := jitterQueue(t)
	q.cfg.MinBackoff = model.Duration(time.Nanosecond)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	attempts, retries := 0, 0
	err := q.sendWriteRequestWithBackoff(ctx, func(int) error {
		attempts++
		return RecoverableError{error: errors.New("retry fixture")}
	}, func() { retries++; cancel() })
	if !errors.Is(err, context.Canceled) || attempts != 1 || retries != 1 {
		t.Fatalf("error=%v attempts=%d retries=%d", err, attempts, retries)
	}
}

func TestHardModeJitterMaintainedHTTP503429Payload(t *testing.T) {
	write := &prompb.WriteRequest{Timeseries: []prompb.TimeSeries{{Labels: []prompb.Label{{Name: "__name__", Value: "agt07_jitter_fixture"}}, Samples: []prompb.Sample{{Timestamp: 1234567890123, Value: 4.25}, {Timestamp: 1234567891123, Value: 5.5}}}}}
	raw, err := write.Marshal()
	if err != nil {
		t.Fatal(err)
	}
	payload := snappy.Encode(nil, raw)
	var mu sync.Mutex
	var bodies [][]byte
	var observed []time.Time
	var handlerError error
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, readErr := io.ReadAll(io.LimitReader(r.Body, 8193))
		defer r.Body.Close()
		mu.Lock()
		defer mu.Unlock()
		if readErr != nil || r.Method != http.MethodPost || r.URL.Path != "/api/v1/write" || len(body) > 8192 {
			handlerError = errors.New("invalid maintained write request")
		}
		bodies = append(bodies, body)
		observed = append(observed, time.Now())
		switch len(bodies) {
		case 1:
			w.Header().Set("Retry-After", "1")
			w.WriteHeader(http.StatusServiceUnavailable)
		case 2:
			w.Header().Set("Retry-After", "1")
			w.WriteHeader(http.StatusTooManyRequests)
		default:
			w.WriteHeader(http.StatusNoContent)
		}
	}))
	defer server.Close()
	u, err := url.Parse(server.URL + "/api/v1/write")
	if err != nil {
		t.Fatal(err)
	}
	client, err := NewWriteClient("agt07-jitter-http", &ClientConfig{URL: &commoncfg.URL{URL: u}, Timeout: model.Duration(time.Second), HTTPClientConfig: commoncfg.DefaultHTTPClientConfig, WriteProtoMsg: remoteapi.WriteV1MessageType, RetryOnRateLimit: true})
	if err != nil {
		t.Fatal(err)
	}
	q := jitterQueue(t)
	wrapped := hardWriteClient{WriteClient: client, h: q.hard}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	retries := 0
	err = q.sendWriteRequestWithBackoff(ctx, func(try int) error { _, err := wrapped.Store(ctx, payload, try); return err }, func() { retries++ })
	if err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	defer mu.Unlock()
	if handlerError != nil || len(bodies) != 3 || retries != 2 {
		t.Fatalf("handler=%v bodies=%d retries=%d", handlerError, len(bodies), retries)
	}
	for i, body := range bodies {
		decoded, err := snappy.Decode(nil, body)
		var received prompb.WriteRequest
		if err != nil || !bytes.Equal(body, payload) || received.Unmarshal(decoded) != nil || !reflect.DeepEqual(received.Timeseries, write.Timeseries) {
			t.Fatalf("body or original timestamps changed on attempt %d", i)
		}
		if i > 0 && observed[i].Sub(observed[i-1]) < time.Second {
			t.Fatal("positive Retry-After lower bound ignored")
		}
	}
}
