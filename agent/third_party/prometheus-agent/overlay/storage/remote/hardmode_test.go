// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// http://www.apache.org/licenses/LICENSE-2.0

package remote

import (
	"context"
	commoncfg "github.com/prometheus/common/config"
	"github.com/prometheus/common/model"
	"github.com/prometheus/prometheus/config"
	remoteapi "github.com/prometheus/client_golang/exp/api/remote"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"sync/atomic"
	"testing"
	"time"

	"github.com/prometheus/prometheus/tsdb/wlog"
)

type hardBlockedClient struct {
	calls   atomic.Int32
	entered chan struct{}
	release chan struct{}
}

func (*hardBlockedClient) Name() string     { return "hard-test" }
func (*hardBlockedClient) Endpoint() string { return "fixture-only" }
func (c *hardBlockedClient) Store(ctx context.Context, _ []byte, _ int) (WriteResponseStats, error) {
	c.calls.Add(1)
	close(c.entered)
	<-ctx.Done()
	<-c.release
	return WriteResponseStats{Samples: 1}, nil // Deliberately late positive ACK.
}

func TestHardModeStoreCancellationJoinsActualClient(t *testing.T) {
	h := wlog.NewHardMode()
	if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	under := &hardBlockedClient{entered: make(chan struct{}), release: make(chan struct{})}
	c := hardWriteClient{WriteClient: under, h: h}
	result := make(chan error, 1)
	go func() { _, err := c.Store(context.Background(), []byte("unchanged-native-payload"), 0); result <- err }()
	select {
	case <-under.entered:
	case <-time.After(time.Second):
		t.Fatal("Store not entered")
	}
	fenced := make(chan struct{})
	go func() { h.Fence(); close(fenced) }()
	select {
	case <-fenced:
		t.Fatal("FD/budget released before client joined")
	case <-time.After(10 * time.Millisecond):
	}
	close(under.release)
	select {
	case err := <-result:
		if err == nil {
			t.Fatal("late ACK became success")
		}
	case <-time.After(time.Second):
		t.Fatal("Store not joined")
	}
	select {
	case <-fenced:
	case <-time.After(time.Second):
		t.Fatal("fence stuck")
	}
	if _, err := c.Store(context.Background(), nil, 1); err == nil || under.calls.Load() != 1 {
		t.Fatal("evicted retry reached client")
	}
}

func TestHardModeMaintainedHttpClientWithheldAck(t *testing.T) {
	h := wlog.NewHardMode()
	if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	received, cancelled := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) != 1 {
			t.Error("unexpected evicted HTTP retry")
		}
		body, err := io.ReadAll(io.LimitReader(r.Body, 256))
		if err != nil || string(body) != "native-transport-opaque-body" || r.Method != "POST" || r.URL.Path != "/api/v1/write" {
			t.Error("maintained client request changed")
		}
		close(received)
		<-r.Context().Done() // Actual received request; deliberately withhold ACK.
		close(cancelled)
		w.WriteHeader(http.StatusNoContent) // May be lost/accepted: never a delivery claim.
	}))
	defer srv.Close()
	defer h.Fail()
	u, err := url.Parse(srv.URL + "/api/v1/write")
	if err != nil {
		t.Fatal(err)
	}
	client, err := NewWriteClient("hard-http", &ClientConfig{URL: &commoncfg.URL{URL: u}, Timeout: model.Duration(5 * time.Second), HTTPClientConfig: commoncfg.DefaultHTTPClientConfig, WriteProtoMsg: remoteapi.WriteV1MessageType})
	if err != nil {
		t.Fatal(err)
	}
	guarded := hardWriteClient{WriteClient: client, h: h}
	result := make(chan error, 1)
	go func() {
		_, err := guarded.Store(context.Background(), []byte("native-transport-opaque-body"), 0)
		result <- err
	}()
	select {
	case <-received:
	case <-time.After(3 * time.Second):
		t.Fatal("real native HTTP request not observed")
	}
	fenced := make(chan struct{})
	go func() { h.Fence(); close(fenced) }()
	select {
	case <-fenced:
	case <-time.After(3 * time.Second):
		t.Fatal("actual HTTP client not joined")
	}
	select {
	case err := <-result:
		if err == nil {
			t.Fatal("withheld/cancelled ACK became success")
		}
	case <-time.After(time.Second):
		t.Fatal("native Store did not return")
	}
	select {
	case <-cancelled:
	case <-time.After(3 * time.Second):
		t.Fatal("actual request context not cancelled")
	}
	if _, err := guarded.Store(context.Background(), nil, 1); err == nil || calls.Load() != 1 {
		t.Fatal("evicted native HTTP retry escaped")
	}
}

func TestHardModeCloseAndFailedReclaimCannotRecreate(t *testing.T) {
	t.Run("close", func(t *testing.T) {
		r := NewStorage(nil, nil, nil, t.TempDir(), time.Second, nil, false)
		defer r.Close()
		h := wlog.NewHardMode()
		r.SetAgentHardMode(h)
		if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
			t.Fatal(err)
		}
		entered, release := make(chan struct{}), make(chan struct{})
		result := make(chan error, 1)
		go func() {
			result <- r.HardReclaim(func() error { close(entered); <-release; return h.Resume(time.Now().Add(time.Hour)) })
		}()
		<-entered
		closing, closed := make(chan struct{}), make(chan struct{})
		go func() { close(closing); r.rws.Close(); close(closed) }()
		<-closing
		select {
		case <-closed:
			t.Fatal("Close raced past active reclaim")
		case <-time.After(10 * time.Millisecond):
		}
		close(release)
		if err := <-result; err != nil {
			t.Fatal(err)
		}
		select {
		case <-closed:
		case <-time.After(time.Second):
			t.Fatal("Close not joined")
		}
		called := false
		if err := r.HardReclaim(func() error { called = true; return nil }); err == nil || called {
			t.Fatal("closed writer invoked deletion callback")
		}
		if err := r.ApplyConfig(&config.Config{}); err == nil {
			t.Fatal("closed writer accepted configuration")
		}
	})
	t.Run("failed-during-fence", func(t *testing.T) {
		r := NewStorage(nil, nil, nil, t.TempDir(), time.Second, nil, false)
		defer r.Close()
		h := wlog.NewHardMode()
		r.SetAgentHardMode(h)
		if err := h.Resume(time.Now().Add(time.Hour)); err != nil {
			t.Fatal(err)
		}
		ctx, finish, err := h.BeginSend(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		called := false
		result := make(chan error, 1)
		go func() { result <- r.HardReclaim(func() error { called = true; return nil }) }()
		select {
		case <-ctx.Done():
		case <-time.After(time.Second):
			t.Fatal("reclaim did not fence")
		}
		h.Fail()
		finish()
		if err := <-result; err == nil || called {
			t.Fatal("failed generation invoked deletion callback")
		}
	})
}
