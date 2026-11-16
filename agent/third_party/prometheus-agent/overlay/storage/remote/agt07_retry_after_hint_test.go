// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// http://www.apache.org/licenses/LICENSE-2.0

package remote

import (
    "context"
    "errors"
    "net/http"
    "net/http/httptest"
    "net/url"
    "strings"
    "sync"
    "sync/atomic"
    "testing"
    "time"

    remoteapi "github.com/prometheus/client_golang/exp/api/remote"
    commoncfg "github.com/prometheus/common/config"
    "github.com/prometheus/common/model"
    "github.com/prometheus/prometheus/tsdb/wlog"
)

func TestHardModeRetryAfterHintArithmetic(t *testing.T) {
    now := time.Date(2026, 11, 13, 0, 0, 0, 500000000, time.UTC)
    rows := []struct{name, raw string; minimum time.Duration; invalid bool}{
        {"max_seconds", "9223372036", 9223372036*time.Second, false},
        {"first_overflow", "9223372037", 0, true},
        {"max_int", "9223372036854775807", 0, true},
        {"over_atoi", "18446744073709551615", 0, true},
        {"huge", strings.Repeat("9", 128), 0, true},
        {"leading_zero_overflow", strings.Repeat("0", 32)+"9223372037", 0, true},
        {"leading_zero_valid", strings.Repeat("0", 32)+"1", time.Second, false},
        {"plus", "+1", time.Second, false},
        {"overlong_numeric", strings.Repeat("9", 4097), 0, true},
        {"zero", "000", 0, false},
        {"negative", "-1", 0, false},
        {"invalid", "not-a-header", 0, false},
        {"invalid_decimal", "12x", 0, false},
        {"past_date", now.Add(-time.Second).Format(http.TimeFormat), 0, false},
        {"subsecond_date", now.Add(500*time.Millisecond).Format(http.TimeFormat), 500*time.Millisecond, false},
        {"future_date", now.Add(1500*time.Millisecond).Format(http.TimeFormat), 1500*time.Millisecond, false},
        {"far_future_date", time.Date(9999, 1, 1, 0, 0, 0, 0, time.UTC).Format(http.TimeFormat), 0, true},
    }
    for _, row := range rows {
        t.Run(row.name, func(t *testing.T) {
            minimum, invalid := hardRetryAfterMinimum(row.raw, now)
            if minimum != row.minimum || invalid != row.invalid { t.Fatalf("minimum=%v invalid=%v", minimum, invalid) }
        })
    }
    original := errors.New("fixed original error")
    wrapped := withHardRetryAfterHint(original, "9223372037")
    if wrapped.Error() != original.Error() || !errors.Is(wrapped, original) { t.Fatal("embedded legacy error changed") }
    if withHardRetryAfterHint(original, "not-a-header") != original { t.Fatal("ordinary invalid fallback changed") }
}

func retryHintMaintainedClient(t *testing.T, endpoint string) WriteClient {
    t.Helper()
    u, err := url.Parse(endpoint)
    if err != nil { t.Fatal(err) }
    client, err := NewWriteClient("agt07-raw-retry", &ClientConfig{URL: &commoncfg.URL{URL:u}, Timeout:model.Duration(time.Second), HTTPClientConfig:commoncfg.DefaultHTTPClientConfig, WriteProtoMsg:remoteapi.WriteV1MessageType, RetryOnRateLimit:true})
    if err != nil { t.Fatal(err) }
    return client
}

func TestHardModeRetryAfterRawOverflowHTTP(t *testing.T) {
    rows := []struct{name, raw string}{
        {"first_overflow", "9223372037"}, {"max_int", "9223372036854775807"},
        {"over_atoi", "18446744073709551615"}, {"huge", strings.Repeat("9",128)},
        {"far_date", time.Date(9999,1,1,0,0,0,0,time.UTC).Format(http.TimeFormat)},
    }
    for _, row := range rows {
        t.Run(row.name, func(t *testing.T) {
            var requests atomic.Int32
            server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
                defer r.Body.Close()
                requests.Add(1)
                w.Header().Set("Retry-After", row.raw)
                w.WriteHeader(http.StatusServiceUnavailable)
            }))
            defer server.Close()
            q := jitterQueue(t)
            wrapped := hardWriteClient{WriteClient:retryHintMaintainedClient(t,server.URL+"/api/v1/write"), h:q.hard}
            ctx, cancel := context.WithTimeout(context.Background(),2*time.Second)
            defer cancel()
            retries := 0
            err := q.sendWriteRequestWithBackoff(ctx,func(try int)error{_,err:=wrapped.Store(ctx,[]byte{0},try);return err},func(){retries++})
            if !errors.Is(err,wlog.ErrHardModeUnavailable) || ctx.Err()!=nil || requests.Load()!=1 || retries!=0 || !q.hard.Failed() { t.Fatalf("err=%v requests=%d retries=%d failed=%v",err,requests.Load(),retries,q.hard.Failed()) }
            attempts := 0
            err=q.sendWriteRequestWithBackoff(context.Background(),func(int)error{attempts++;return nil},func(){retries++})
            if !errors.Is(err,wlog.ErrHardModeUnavailable) || attempts!=0 || requests.Load()!=1 { t.Fatal("new WAL batch bypassed sticky receiver minimum") }
        })
    }
}

func TestHardModeRetryAfterHTTPDateMinimum(t *testing.T) {
    var target time.Time
    var mu sync.Mutex
    var observed []time.Time
    server:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){
        defer r.Body.Close()
        mu.Lock();defer mu.Unlock()
        observed=append(observed,time.Now())
        if len(observed)==1 {target=time.Now().Add(2500*time.Millisecond).UTC().Truncate(time.Second);w.Header().Set("Retry-After",target.Format(http.TimeFormat));w.WriteHeader(http.StatusServiceUnavailable)}else{w.WriteHeader(http.StatusNoContent)}
    }))
    defer server.Close()
    q:=jitterQueue(t)
    wrapped:=hardWriteClient{WriteClient:retryHintMaintainedClient(t,server.URL+"/api/v1/write"),h:q.hard}
    ctx,cancel:=context.WithTimeout(context.Background(),5*time.Second);defer cancel()
    retries:=0
    err:=q.sendWriteRequestWithBackoff(ctx,func(try int)error{_,err:=wrapped.Store(ctx,[]byte{0},try);return err},func(){retries++})
    if err!=nil{t.Fatal(err)}
    mu.Lock();defer mu.Unlock()
    if len(observed)!=2 || retries!=1 || !observed[0].Before(target.Add(-500*time.Millisecond)) || observed[1].Before(target) {t.Fatal("actual future HTTP-date minimum not demonstrated")}
}
