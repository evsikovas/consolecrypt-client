// Copyright The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// http://www.apache.org/licenses/LICENSE-2.0

package remote

import (
    "net/http"
    "time"
)

// Only hard-mode scheduling reads this hint. Embedded error text and legacy
// RecoverableError's two fields/duration parser remain unchanged.
type hardRetryAfterHint struct {
    error
    minimum time.Duration
    invalid bool
}

func (h *hardRetryAfterHint) Unwrap() error { return h.error }

func withHardRetryAfterHint(original error, raw string) error {
    minimum, invalid := hardRetryAfterMinimum(raw, time.Now())
    if invalid || minimum > 0 {
        return &hardRetryAfterHint{error: original, minimum: minimum, invalid: invalid}
    }
    return original
}

// Explicit-now is pure arithmetic for tests; production calls fixed time.Now.
// Numeric parsing is allocation-free and bounded. Oversized numeric-looking
// headers are hard-mode invalid, never an immediate fallback retry.
func hardRetryAfterMinimum(raw string, now time.Time) (time.Duration, bool) {
    const maxBytes = 4096
    const maxSeconds = "9223372036" // floor(MaxInt64 nanoseconds / second).
    if len(raw) == 0 { return 0, false }
    if len(raw) > maxBytes {
        if (raw[0] >= '0' && raw[0] <= '9') || raw[0] == '+' { return 0, true }
        return 0, false // Ordinary non-numeric invalid header, legacy fallback.
    }
    decimal := raw
    if decimal[0] == '+' { decimal = decimal[1:] } // Existing Atoi semantics.
    numeric := len(decimal) > 0
    for i := 0; i < len(decimal); i++ {
        if decimal[i] < '0' || decimal[i] > '9' { numeric = false; break }
    }
    if numeric {
        i := 0
        for i < len(decimal) && decimal[i] == '0' { i++ }
        decimal = decimal[i:]
        if len(decimal) == 0 { return 0, false }
        if len(decimal) > len(maxSeconds) || (len(decimal) == len(maxSeconds) && decimal > maxSeconds) { return 0, true }
        var seconds int64
        for i := 0; i < len(decimal); i++ { seconds = seconds*10 + int64(decimal[i]-'0') }
        return time.Duration(seconds) * time.Second, false
    }
    parsed, err := time.Parse(http.TimeFormat, raw) // Same date grammar as legacy.
    if err != nil || !parsed.After(now) { return 0, false }
    // Sub saturates: compare the absolute endpoint BEFORE calling it.
    if now.Add(time.Duration(1<<63-1)).Before(parsed) { return 0, true }
    return parsed.Sub(now), false // Preserve subsecond future-date minimum.
}
