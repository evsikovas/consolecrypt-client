//go:build linux

// Copyright 2026 ConsoleCrypt contributors.
// Licensed under the Apache License, Version 2.0 (the Prometheus source license).

package agent

// TEST ONLY additive opt-in to the existing Agent test ELF. It runs the REAL
// scrape Manager -> Agent WAL -> remote.Storage pipeline; no manual Append,
// replacement codec, receiver, ACK ledger, clock or production test hook.
import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/common/model"
	"github.com/prometheus/common/promslog"

	"github.com/prometheus/prometheus/config"
	"github.com/prometheus/prometheus/discovery/targetgroup"
	"github.com/prometheus/prometheus/scrape"
	"github.com/prometheus/prometheus/storage/remote"
	"github.com/prometheus/prometheus/tsdb/wlog"
)

type ccAC19Config struct {
	Format       string `json:"format"`
	Token        string `json:"token"`
	Root         string `json:"root"`
	ExporterPort int    `json:"exporterPort"`
	ReceiverPort int    `json:"receiverPort"`
}

type ccAC19Marker struct {
	Kind      string `json:"kind"`
	Timestamp int64  `json:"timestamp"`
}

type ccAC19Phase struct {
	Format  string         `json:"format"`
	Token   string         `json:"token"`
	Markers []ccAC19Marker `json:"markers"`
}

type ccAC19Stop struct {
	Format string `json:"format"`
	Token  string `json:"token"`
}

// All control files belong to the sole fixture writer, outside the Agent queue.
// This bounded, private TEST channel is never interpreted as grant authority.
func ccAC19Read(t *testing.T, path string, result any, optional bool) bool {
	t.Helper()
	info, err := os.Lstat(path)
	if os.IsNotExist(err) && optional {
		return false
	}
	if err != nil || !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > 16384 || info.Mode().Perm() != 0600 {
		t.Fatal("invalid owned AC19 control file")
	}
	owner, ok := info.Sys().(*syscall.Stat_t)
	if !ok || owner.Uid != 10001 || owner.Nlink != 1 {
		t.Fatal("invalid owned AC19 control owner")
	}
	f, err := os.Open(path)
	if err != nil {
		t.Fatal("cannot open owned AC19 control")
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 16385))
	if err != nil || len(raw) > 16384 {
		t.Fatal("cannot read bounded AC19 control")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if decoder.Decode(result) != nil {
		t.Fatal("invalid AC19 control schema")
	}
	var extra any
	if decoder.Decode(&extra) != io.EOF {
		t.Fatal("extra AC19 control frame")
	}
	// Python emits exactly this canonical field order/ASCII representation.
	// This also rejects duplicate, case-folded or alternate key spellings.
	canonical, err := json.Marshal(result)
	if err != nil || !bytes.Equal(raw, canonical) {
		t.Fatal("noncanonical AC19 control")
	}
	return true
}

func ccAC19Publish(t *testing.T, root, name string, value any) {
	t.Helper()
	raw, err := json.Marshal(value)
	if err != nil || len(raw) > 16384 {
		t.Fatal("AC19 status exceeded bound")
	}
	file, err := os.OpenFile(filepath.Join(root, "."+name+".pending"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal("cannot create owned AC19 observation")
	}
	if _, err = file.Write(raw); err != nil {
		file.Close()
		t.Fatal("cannot write owned AC19 observation")
	}
	if err = file.Sync(); err != nil {
		file.Close()
		t.Fatal("cannot sync owned AC19 observation")
	}
	if file.Close() != nil || os.Rename(file.Name(), filepath.Join(root, name)) != nil {
		t.Fatal("cannot publish owned AC19 observation")
	}
}

func ccAC19Metrics(t *testing.T, registry *prometheus.Registry) map[string]float64 {
	t.Helper()
	metrics := map[string]float64{}
	families, err := registry.Gather()
	if err != nil {
		t.Fatal("cannot gather actual Agent metrics")
	}
	for _, family := range families {
		name := family.GetName()
		if !strings.HasPrefix(name, "prometheus_agent_hard_") && name != "prometheus_agent_samples_appended_total" {
			continue
		}
		for _, metric := range family.Metric {
			if metric.Counter != nil {
				metrics[name] += metric.Counter.GetValue()
			}
			if metric.Gauge != nil {
				metrics[name] += metric.Gauge.GetValue()
			}
		}
	}
	for _, name := range []string{"charged_bytes", "oldest_segment_residence_seconds", "evicted_wal_bytes_total", "evicted_segments_total", "loss_unknown", "unavailable"} {
		if _, ok := metrics["prometheus_agent_hard_"+name]; !ok {
			t.Fatal("required native hardmode metric absent")
		}
	}
	if metrics["prometheus_agent_hard_unavailable"] != 0 || metrics["prometheus_agent_hard_charged_bytes"] > 536870912 {
		t.Logf("native hardmode failure raw fixed metrics: %v", metrics)
		t.Fatal("native hardmode unavailable or charged cap exceeded")
	}
	return metrics
}

func ccAC19ScrapeConfig(t *testing.T, c ccAC19Config, quiet bool) *config.Config {
	t.Helper()
	interval, timeout := "20ms", "20ms"
	if quiet {
		interval, timeout = "15s", "1s"
	}
	text := fmt.Sprintf(`global:
  scrape_interval: %s
  scrape_timeout: %s
scrape_configs:
- job_name: cc_ac19
  honor_timestamps: true
  sample_limit: 224
  body_size_limit: 256KB
  static_configs:
  - targets: ["127.0.0.1:%d"]
remote_write:
- name: cc_ac19
  url: "http://127.0.0.1:%d/api/v1/write"
  remote_timeout: 1s
  follow_redirects: false
  proxy_from_environment: false
  enable_http2: false
  send_exemplars: false
  send_native_histograms: false
  metadata_config:
    send: false
  queue_config:
    capacity: 1024
    min_shards: 1
    max_shards: 1
    max_samples_per_send: 256
    batch_send_deadline: 1s
    min_backoff: 100ms
    max_backoff: 1s
    sample_age_limit: 0s
`, interval, timeout, c.ExporterPort, c.ReceiverPort)
	conf, err := config.Load(text, promslog.NewNopLogger())
	if err != nil {
		t.Fatal("real Agent configuration rejected")
	}
	return conf
}

func TestCCOwnedHardmodeRealAC19(t *testing.T) {
	if os.Getenv("CC_PROM_HARDMODE_AC19") == "" {
		t.Skip("requires explicit owned REAL10800s outage fixture")
	}
	if os.Getenv("CC_PROM_HARDMODE_AC19") != "1" || os.Getuid() != 10001 {
		t.Fatal("invalid explicit AC19 opt-in or UID")
	}
	root := os.Getenv("CC_PROM_HARDMODE_OWNED_ROOT")
	var c ccAC19Config
	ccAC19Read(t, filepath.Join(root, "native-config.json"), &c, false)
	validToken := regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`)
	if c.Format != "cc-prom-hardmode-ac19-config/v1" || !validToken.MatchString(c.Token) || c.Token == "00000000-0000-0000-0000-000000000000" || c.Root != root || !filepath.IsAbs(root) || filepath.Clean(root) != root || filepath.Dir(root) != "/tmp" || filepath.Base(root) != "cc-prom-hardmode-owned-"+c.Token {
		t.Fatal("invalid exact owned AC19 root")
	}
	info, err := os.Lstat(root)
	if err != nil || !info.IsDir() || info.Mode().Perm() != 0700 {
		t.Fatal("invalid private AC19 root")
	}
	for _, port := range []int{c.ExporterPort, c.ReceiverPort} {
		if port < 1024 || port > 65535 {
			t.Fatal("invalid owned loopback port")
		}
	}
	if c.ExporterPort == c.ReceiverPort {
		t.Fatal("overlapping AC19 endpoints")
	}
	// Queue MUST be a real Linux writable overlay, never a tmpfs substitute.
	mounts, err := os.ReadFile("/proc/self/mountinfo")
	if err != nil {
		t.Fatal("missing Linux filesystem provenance")
	}
	longest, filesystem := 0, ""
	for _, line := range strings.Split(string(mounts), "\n") {
		parts := strings.SplitN(line, " - ", 2)
		if len(parts) != 2 {
			continue
		}
		fields, after := strings.Fields(parts[0]), strings.Fields(parts[1])
		if len(fields) < 5 || len(after) == 0 {
			continue
		}
		mount := fields[4]
		if (mount == "/" || root == mount || strings.HasPrefix(root, mount+"/")) && len(mount) > longest {
			longest, filesystem = len(mount), after[0]
		}
	}
	if filesystem != "overlay" {
		t.Fatal("AC19 queue requires owned real overlay filesystem")
	}
	// Registered before closePipeline: testing cleanup is LIFO, so this
	// bounded ERROR dump runs AFTER native pipeline cleanup, even on Fatal.
	errorLog := &ccReclaimLog{}
	errorLogger := slog.New(slog.NewTextHandler(errorLog, &slog.HandlerOptions{Level: slog.LevelError}))
	t.Cleanup(func() { errorLog.print(t) })
	registry := prometheus.NewRegistry()
	queue := filepath.Join(root, "queue")
	if os.Mkdir(queue, 0700) != nil {
		t.Fatal("cannot initialize fresh owned native queue directory")
	}
	marker, err := os.OpenFile(filepath.Join(queue, "lock"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal("cannot provision owned hardmode layout marker")
	}
	if _, err = marker.WriteString(hardLayoutMagic); err != nil {
		marker.Close()
		t.Fatal("cannot write owned layout marker")
	}
	if err = marker.Close(); err != nil {
		t.Fatal("cannot close owned layout marker")
	}

	rs := remote.NewStorage(errorLogger, registry, func() (int64, error) { return 0, nil }, queue, time.Second, nil, false)
	rs.SetAgentHardMode(wlog.NewHardMode())
	var db *DB
	var manager *scrape.Manager
	runDone := make(chan error, 1)
	running := false
	closed := false
	closePipeline := func() {
		if closed {
			return
		}
		closed = true
		if manager != nil {
			manager.Stop()
		}
		if running {
			select {
			case err := <-runDone:
				if err != nil {
					t.Error("real scrape Manager exited with error")
				}
			case <-time.After(10 * time.Second):
				t.Error("real scrape Manager did not join")
			}
		}
		// Native fanout order: healthy primary closes before secondary abort.
		if db != nil && db.Close() != nil {
			t.Error("real Agent WAL did not close cleanly")
		}
		if rs.Close() != nil {
			t.Error("real remote storage did not close cleanly")
		}

	}
	t.Cleanup(closePipeline)
	opts := DefaultOptions()
	opts.HardBudget = true
	db, err = Open(errorLogger, registry, rs, queue, opts)
	if err != nil {
		t.Fatal("actual hard Agent Open rejected")
	}
	db.SetWriteNotified(rs)
	manager, err = scrape.NewManager(nil, errorLogger, nil, db, nil, registry)
	if err != nil {
		t.Fatal("actual scrape Manager constructor rejected")
	}
	conf := ccAC19ScrapeConfig(t, c, false)
	if rs.ApplyConfig(conf) != nil || manager.ApplyConfig(conf) != nil {
		t.Fatal("actual initial scrape/remote configuration rejected")
	}
	targets := make(chan map[string][]*targetgroup.Group, 1)
	targets <- map[string][]*targetgroup.Group{"cc_ac19": {{Source: "owned-static", Targets: []model.LabelSet{{model.AddressLabel: model.LabelValue(fmt.Sprintf("127.0.0.1:%d", c.ExporterPort))}}}}}
	running = true
	go func() { runDone <- manager.Run(targets) }()
	started := time.Now()
	quiet := false
	quietPaused, recentResumed := false, false
	// Bounded sticky TEST observations of actual completed commits. These are
	// not production replay/ACK records or delivery authority.
	admitted := map[string]map[string]any{}
	expected := map[string]int64{}
	for time.Since(started) < 11880*time.Second {
		select {
		case <-runDone:
			running = false
			t.Fatal("actual scrape Manager stopped before explicit fixture stop")
		default:
		}
		metrics := ccAC19Metrics(t, registry)
		for _, phase := range []string{"quiet", "recent"} {
			var request ccAC19Phase
			if ccAC19Read(t, filepath.Join(root, phase+".json"), &request, true) {
				if request.Format != "cc-prom-hardmode-ac19-"+phase+"/v1" || request.Token != c.Token || len(request.Markers) != 3 {
					t.Fatal("invalid exact AC19 phase")
				}
				if phase == "recent" && !quietPaused {
					t.Fatal("recent phase before actual quiet pause")
				}
				if phase == "quiet" && !quiet {
					if metrics["prometheus_agent_hard_evicted_segments_total"] < 1 {
						t.Fatal("quiet phase cannot substitute for actual byte pressure")
					}
					if manager.ApplyConfig(ccAC19ScrapeConfig(t, c, true)) != nil {
						t.Fatal("actual quiet scrape interval reload rejected")
					}
					quiet = true
				}
				if phase == "recent" && !recentResumed {
					if manager.ApplyConfig(ccAC19ScrapeConfig(t, c, true)) != nil {
						t.Fatal("actual recent scrape resume rejected")
					}
					// ApplyConfig doesn't recreate a removed pool. The same
					// owned static discovery update triggers native reload.
					select {
					case targets <- map[string][]*targetgroup.Group{"cc_ac19": {{Source: "owned-static", Targets: []model.LabelSet{{model.AddressLabel: model.LabelValue(fmt.Sprintf("127.0.0.1:%d", c.ExporterPort))}}}}}:
					case <-runDone:
						running = false
						t.Fatal("actual scrape Manager stopped before recent resume")
					case <-time.After(5 * time.Second):
						t.Fatal("actual recent static target update blocked")
					}
					recentResumed = true
				}
				kinds := map[string]bool{}
				for _, marker := range request.Markers {
					if (marker.Kind != "normal" && marker.Kind != "old" && marker.Kind != "future") || kinds[marker.Kind] || marker.Timestamp <= 0 {
						t.Fatal("invalid AC19 marker descriptor")
					}
					kinds[marker.Kind] = true
					key := phase + "/" + marker.Kind
					if old, ok := expected[key]; ok && old != marker.Timestamp {
						t.Fatal("AC19 marker expectations changed")
					}
					expected[key] = marker.Timestamp
				}
			}
		}
		for series := range db.series.allSeries() {
			l := series.Labels()
			if l.Get("__name__") != "cc_ac19_marker" {
				continue
			}
			key := l.Get("phase") + "/" + l.Get("kind")
			if timestamp, ok := expected[key]; ok && series.LastSampleTimestamp() == timestamp {
				if _, ok := admitted[key]; !ok {
					admitted[key] = map[string]any{"source_timestamp_ms": timestamp, "observed_elapsed_seconds": time.Since(started).Seconds()}
				}
			}
		}
		if quiet && !quietPaused && admitted["quiet/normal"] != nil && admitted["quiet/old"] != nil && admitted["quiet/future"] != nil {
			// A sparse live target still generates up/scrape samples. Once
			// the real marker commits are observed, stop/join only scraping
			// to isolate physical age from byte pressure. Agent/RS stay live.
			paused := ccAC19ScrapeConfig(t, c, true)
			paused.ScrapeConfigs = nil
			if manager.ApplyConfig(paused) != nil {
				t.Fatal("actual empty scrape configuration rejected")
			}
			quietPaused = true
			// Native stop can append one final stale/report batch. Snapshot
			// the byte/age baseline only AFTER all pool loops have joined.
			metrics = ccAC19Metrics(t, registry)
		}
		ccAC19Publish(t, root, "native-status.json", map[string]any{"format": "cc-prom-hardmode-ac19-status/v1", "token": c.Token,
			"ready": true, "elapsed_seconds": time.Since(started).Seconds(), "quiet_applied": quiet,
			"quiet_paused": quietPaused, "recent_resumed": recentResumed,
			"filesystem": filesystem, "metrics": metrics, "admitted_markers": admitted})
		var stop ccAC19Stop
		if ccAC19Read(t, filepath.Join(root, "stop.json"), &stop, true) {
			if stop.Format != "cc-prom-hardmode-ac19-stop/v1" || stop.Token != c.Token || time.Since(started) < 10800*time.Second || !quietPaused || !recentResumed || len(admitted) != 6 {
				t.Fatal("early or incomplete REAL AC19 stop")
			}
			closePipeline()
			ccAC19Publish(t, root, "native-final.json", map[string]any{"format": "cc-prom-hardmode-ac19-final/v1", "token": c.Token,
				"passed": !t.Failed(), "elapsed_seconds": time.Since(started).Seconds(), "admitted_markers": admitted,
				"pipeline_closed": !t.Failed(), "production_enabled": false})
			return
		}
		time.Sleep(time.Second)
	}
	t.Fatal("REAL AC19 native test deadline, not a retention PASS")
}
