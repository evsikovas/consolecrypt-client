import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/usage.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart' show WslcApi;
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart' show Color, ThemeMode;

enum DeskSection { containers, images, storage, networks, topology, cleanup, settings }

const inventorySections = [DeskSection.containers, DeskSection.images, DeskSection.storage, DeskSection.networks];

class DeskController extends ChangeNotifier {
  DeskController({required this.api, required this.l10n});
  AppLocalizations l10n;
  final WslcApi api;
  RuntimeInfo? runtime;
  List<ContainerSummary> containers = const [];
  List<ImageSummary> images = const [];
  List<VolumeSummary> volumes = const [];
  List<NetworkSummary> networks = const [];

  /// Sections read at least once. Others show a loading state instead of "empty".
  final Set<DeskSection> loaded = {};
  DeskSection section = DeskSection.containers;
  String? openedContainerId;
  String? error;
  bool connecting = false;
  bool stale = false;
  bool suspended = false;
  final Set<String> busyIds = {};
  int refreshSeconds = 5;

  // Values that change on every poll live in notifiers, so a tick rebuilds only the
  // widgets that show them instead of the whole shell.
  final syncing = ValueNotifier<bool>(false);
  final lastSync = ValueNotifier<DateTime?>(null);
  final themeMode = ValueNotifier<ThemeMode>(ThemeMode.system);
  final accentColor = ValueNotifier<Color?>(null);
  final backgroundTint = ValueNotifier<Color?>(null);
  final notificationsEnabled = ValueNotifier<bool>(true);

  /// Closing the window hides it in the notification area instead of quitting.
  final closeToTray = ValueNotifier<bool>(false);

  /// Whether Windows starts the app at sign-in; null until read.
  final autostart = ValueNotifier<bool?>(null);

  /// A `wslc.exe` chosen in settings; null means the default location.
  String? cliPath;

  /// Saved wizard settings, sorted by name.
  List<ContainerTemplate> templates = const [];
  bool _settingsReady = false;
  Future<void> _saving = Future.value();

  /// Reads saved settings, then connects with them.
  Future<void> start() async {
    await loadSettings();
    if (_disposed) return;
    await connect(cliPath);
  }

  Future<void> loadSettings() async {
    try {
      final s = await api.loadSettings();
      if (_disposed) return;
      themeMode.value = switch (s.theme) {
        'light' => ThemeMode.light,
        'dark' => ThemeMode.dark,
        _ => ThemeMode.system,
      };
      accentColor.value = parseHexColor(s.accentColor);
      backgroundTint.value = parseHexColor(s.backgroundTint);
      if (const [2, 5, 10].contains(s.refreshSeconds)) {
        refreshSeconds = s.refreshSeconds!;
      }
      notificationsEnabled.value = s.notifications ?? true;
      closeToTray.value = s.closeToTray ?? false;
      cliPath = s.cliPath;
      templates = s.templates;
    } catch (_) {
      // Defaults; the next change writes a fresh file.
    }
    if (_disposed || _settingsReady) return;
    _settingsReady = true;
    for (final n in [themeMode, notificationsEnabled, closeToTray]) {
      n.addListener(_saveQuietly);
    }
    notifyListeners();
  }

  DeskSettings get settings => DeskSettings(
    theme: themeMode.value.name,
    accentColor: accentColor.value == null ? null : colorToHex(accentColor.value!),
    backgroundTint: backgroundTint.value == null ? null : colorToHex(backgroundTint.value!),
    refreshSeconds: refreshSeconds,
    notifications: notificationsEnabled.value,
    closeToTray: closeToTray.value,
    cliPath: cliPath,
    templates: templates,
  );

  /// Writes settings one save at a time. Returns an error text or null.
  Future<String?> saveSettings() async {
    if (!_settingsReady) return null;
    final snapshot = settings;
    final run = _saving.then((_) => api.saveSettings(settings: snapshot));
    _saving = run.catchError((Object _) {});
    try {
      await run;
      return null;
    } catch (e) {
      return errorText(e);
    }
  }

  void _saveQuietly() => unawaited(saveSettings());

  Future<String?> setAppearanceColor(Color? color, {required bool background}) {
    (background ? backgroundTint : accentColor).value = color;
    return saveSettings();
  }

  /// Adds or replaces the template with this name.
  Future<String?> saveTemplate(String name, RunSpec spec) {
    templates = [
      for (final t in templates)
        if (t.name != name) t,
      ContainerTemplate(name: name, spec: spec),
    ]..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    notifyListeners();
    return saveSettings();
  }

  Future<String?> deleteTemplate(String name) {
    templates = [
      for (final t in templates)
        if (t.name != name) t,
    ];
    notifyListeners();
    return saveSettings();
  }

  Future<void> loadAutostart() async {
    try {
      autostart.value = await api.autostartEnabled();
    } catch (_) {
      autostart.value = false;
    }
  }

  /// Returns an error text or null on success.
  Future<String?> setAutostart(bool enabled) async {
    try {
      await api.setAutostart(enabled: enabled);
      autostart.value = enabled;
      return null;
    } catch (e) {
      return errorText(e);
    }
  }

  /// Containers changed from this app recently; their exits are expected.
  final Map<String, DateTime> _touched = {};

  bool touchedRecently(String id) {
    final at = _touched[id];
    return at != null && DateTime.now().difference(at).inSeconds < 90;
  }

  void _touch(Iterable<String> ids) {
    final now = DateTime.now();
    for (final id in ids) {
      _touched[id] = now;
    }
    _touched.removeWhere((_, at) => now.difference(at).inMinutes > 5);
  }

  /// Resource usage of running containers by full ID. `wslc stats` samples for ~2 s, so it
  /// is polled separately and never delays the list.
  final stats = ValueNotifier<Map<String, ContainerStats>>(const {});
  List<HostAddress> hostAddresses = const [];

  /// Registries logged into from this app in this run (the CLI cannot list them).
  final Set<String?> registries = {};
  bool _statsBusy = false;

  /// Recent samples per container for charts; updated right before [stats].
  final Map<String, UsageHistory> history = {};

  Timer? _poller;
  int _tick = 0;
  Future<void>? _running;
  Future<void>? _next;
  final Set<DeskSection> _nextTargets = {};
  bool _disposed = false;

  /// True while `wslc events` streams changes; polling then only backs it up.
  bool eventsLive = false;
  Timer? _eventsTimer;
  Timer? _debounce;
  bool _eventsBusy = false;
  DateTime _eventsStarted = DateTime.fromMillisecondsSinceEpoch(0);
  final Set<DeskSection> _dirty = {};
  final _events = StreamController<DeskEvent>.broadcast();

  /// Runtime events as they arrive (used by notifications).
  Stream<DeskEvent> get events => _events.stream;

  bool get ready => runtime?.readable == true;
  bool get canMutate => ready && !stale && error == null;

  Future<void> connect(String? path) async {
    if (connecting) return;
    _poller?.cancel();
    connecting = true;
    syncing.value = true;
    error = null;
    notifyListeners();
    try {
      final info = await api.probeRuntime(cliPath: path);
      if (_disposed) return;
      runtime = info;
      // Remember a path that works, and forget a custom one once the default works.
      if (info.readable && path != cliPath) {
        cliPath = path;
        _saveQuietly();
      }
      if (!info.readable) {
        error = info.error == null ? l10n.wslcRuntimeUnavailable : errorText(info.error!);
        stale = loaded.isNotEmpty;
      }
    } catch (e) {
      runtime = null;
      error = errorText(e);
      stale = loaded.isNotEmpty;
    } finally {
      if (!_disposed) {
        connecting = false;
        syncing.value = false;
      }
    }
    if (_disposed) return;
    notifyListeners();
    unawaited(_loadAddresses());
    // The probe has already warmed up the WSLc session; read every section in parallel.
    if (ready) {
      await refresh(all: true);
      await _startEvents();
    }
    _schedule();
  }

  Future<void> _loadAddresses() async {
    try {
      hostAddresses = await loadHostAddresses();
    } catch (_) {
      hostAddresses = const [];
    }
  }

  Future<void> _startEvents() async {
    if (_disposed) return;
    _eventsStarted = DateTime.now();
    try {
      await api.startEvents();
    } catch (_) {
      // Without events the poller keeps the data fresh on its own.
    }
    _eventsTimer ??= Timer.periodic(const Duration(milliseconds: 400), (_) => unawaited(_pollEvents()));
  }

  static DeskSection? sectionOf(String kind) => switch (kind) {
    'container' => DeskSection.containers,
    'image' => DeskSection.images,
    'volume' => DeskSection.storage,
    'network' => DeskSection.networks,
    _ => null,
  };

  Future<void> _pollEvents() async {
    if (_eventsBusy || _disposed || !ready) return;
    _eventsBusy = true;
    try {
      final batch = await api.takeEvents();
      if (_disposed) return;
      if (batch.running != eventsLive) {
        eventsLive = batch.running;
        notifyListeners();
        _schedule();
      }
      if (!batch.running && DateTime.now().difference(_eventsStarted).inSeconds >= 10) {
        unawaited(_startEvents());
      }
      for (final e in batch.events) {
        final target = sectionOf(e.kind);
        if (target != null) _dirty.add(target);
        // A removed container can free images and volumes, a pull adds an image.
        if (e.kind == 'container' && e.action == 'destroy') {
          _dirty.addAll(inventorySections);
        }
        _events.add(e);
      }
      if (_dirty.isNotEmpty) {
        _debounce?.cancel();
        _debounce = Timer(const Duration(milliseconds: 200), () {
          final targets = {..._dirty};
          _dirty.clear();
          unawaited(refreshSections(targets));
        });
      }
    } catch (_) {
      // The next poll retries.
    } finally {
      _eventsBusy = false;
    }
  }

  void _pollStats() {
    if (_statsBusy || _disposed || !ready) return;
    if (section != DeskSection.containers || !containers.any((c) => c.state == ContainerState.running)) {
      if (stats.value.isNotEmpty) stats.value = const {};
      return;
    }
    _statsBusy = true;
    api
        .containerStats()
        .then((rows) {
          if (_disposed) return;
          final now = DateTime.now();
          for (final r in rows) {
            history.putIfAbsent(r.id, UsageHistory.new).add(r, now);
          }
          final alive = {for (final c in containers) c.id};
          history.removeWhere((id, _) => !alive.contains(id));
          stats.value = {for (final r in rows) r.id: r};
        })
        .catchError((Object _) {
          // Usage is informational; a failed sample keeps the previous values.
        })
        .whenComplete(() => _statsBusy = false);
  }

  void _schedule() {
    _poller?.cancel();
    if (_disposed) return;
    _poller = Timer.periodic(Duration(seconds: refreshSeconds), (_) {
      if (suspended) return;
      _tick++;
      final fullTick = _tick * refreshSeconds % 30 == 0;
      if (eventsLive) {
        // Events keep lists current; usage still needs sampling, and a full read twice a
        // minute guards against a missed event.
        _pollStats();
        if (fullTick) unawaited(refresh(all: true));
      } else {
        // The active section and container counters every tick, the rest twice a minute.
        unawaited(refresh(all: fullTick));
      }
    });
  }

  void setRefreshSeconds(int seconds) {
    if (seconds == refreshSeconds) return;
    refreshSeconds = seconds;
    notifyListeners();
    _saveQuietly();
    if (_poller != null) _schedule();
  }

  Set<DeskSection> get _activeTargets => {DeskSection.containers, if (section != DeskSection.settings) section};

  Future<void> refresh({bool all = false}) => refreshSections(all ? inventorySections : _activeTargets);

  /// Concurrent callers share one in-flight read and at most one queued follow-up.
  Future<void> refreshSections(Iterable<DeskSection> targets) {
    if (_disposed || !ready) return Future.value();
    final wanted = targets.toSet();
    if (_running != null) {
      _nextTargets.addAll(wanted);
      return _next ??= _running!.then((_) {
        final queued = {..._nextTargets};
        _nextTargets.clear();
        _next = null;
        return refreshSections(queued);
      });
    }
    final run = _refresh(wanted).whenComplete(() => _running = null);
    _running = run;
    return run;
  }

  Future<void> _refresh(Set<DeskSection> targets) async {
    syncing.value = true;
    final failures = <String>[];
    var changed = false;
    await Future.wait(
      targets.map((target) async {
        try {
          if (await _load(target)) changed = true;
        } catch (e) {
          failures.add(errorText(e));
        }
      }),
    );
    if (_disposed) return;
    final nextError = failures.isEmpty ? null : failures.first;
    final nextStale = failures.isNotEmpty && loaded.isNotEmpty;
    if (nextError != error || nextStale != stale) {
      error = nextError;
      stale = nextStale;
      changed = true;
    }
    if (failures.isEmpty) lastSync.value = DateTime.now();
    syncing.value = false;
    if (changed) notifyListeners();
    _pollStats();
  }

  /// Returns true when the stored data actually changed.
  Future<bool> _load(DeskSection target) async {
    final first = !loaded.contains(target);
    switch (target) {
      case DeskSection.containers:
        final rows = await api.listContainers();
        if (!first && listEquals(rows, containers)) return false;
        containers = rows;
      case DeskSection.images:
        final rows = await api.listImages();
        if (!first && listEquals(rows, images)) return false;
        images = rows;
      case DeskSection.storage:
        final rows = await api.listVolumes();
        if (!first && listEquals(rows, volumes)) return false;
        volumes = rows;
      case DeskSection.networks:
        final rows = await api.listNetworks();
        if (!first && listEquals(rows, networks)) return false;
        networks = rows;
      case DeskSection.topology:
      case DeskSection.cleanup:
      case DeskSection.settings:
        return false;
    }
    loaded.add(target);
    return true;
  }

  void selectSection(DeskSection value) {
    if (section == value && openedContainerId == null) return;
    section = value;
    openedContainerId = null;
    notifyListeners();
    unawaited(refresh());
  }

  void openContainer(String id) {
    section = DeskSection.containers;
    openedContainerId = id;
    notifyListeners();
  }

  void closeContainer() {
    openedContainerId = null;
    notifyListeners();
  }

  void setThemeMode(ThemeMode mode) => themeMode.value = mode;

  Future<OperationResult?> operate(String id, String action) async {
    if (!canMutate || busyIds.contains(id)) return null;
    busyIds.add(id);
    _touch([id]);
    notifyListeners();
    OperationResult? result;
    try {
      result = await switch (action) {
        'start' => api.startContainer(containerId: id),
        'stop' => api.stopContainer(containerId: id, confirmed: true),
        'restart' => api.restartContainer(containerId: id, confirmed: true),
        _ => throw ArgumentError(l10n.wslcUnknownAction),
      };
      await refresh();
      if (result.uncertain) {
        // Keep mutations disabled until the next successful read; never retry automatically.
        error = l10n.wslcCheckTheResultOfTheCommandWillNot(
          _actionLabel(l10n, action),
          result.commandError?.message ?? result.verificationError?.message ?? '',
        );
        stale = true;
      }
    } catch (e) {
      error = errorText(e);
      stale = true;
    } finally {
      busyIds.remove(id);
      if (!_disposed) notifyListeners();
    }
    return result;
  }

  /// Applies [action] (start, stop, restart or remove) to [rows], at most three at a time,
  /// then reads the list once. Never retries; uncertain results pause mutations like
  /// single operations do. Returns the number of successes and failure texts.
  Future<(int, List<String>)> bulk(List<ContainerSummary> rows, String action) async {
    if (!canMutate || rows.isEmpty) return (0, const <String>[]);
    final queue = [...rows];
    final failures = <String>[];
    var done = 0;
    var uncertain = false;
    busyIds.addAll(rows.map((r) => r.id));
    _touch(rows.map((r) => r.id));
    notifyListeners();
    Future<void> worker() async {
      while (queue.isNotEmpty) {
        final r = queue.removeAt(0);
        final label = r.name ?? r.id.substring(0, 12);
        try {
          if (action == 'remove') {
            await api.removeContainer(containerId: r.id, force: r.state == ContainerState.running);
          } else {
            final result = await switch (action) {
              'start' => api.startContainer(containerId: r.id),
              'stop' => api.stopContainer(containerId: r.id, confirmed: true),
              'restart' => api.restartContainer(containerId: r.id, confirmed: true),
              _ => throw ArgumentError(l10n.wslcUnknownAction),
            };
            if (result.uncertain) {
              uncertain = true;
              failures.add(l10n.wslcCheckTheResult(label));
              continue;
            }
          }
          done++;
        } catch (e) {
          failures.add('$label: ${e is AppError ? e.message : e}');
        } finally {
          busyIds.remove(r.id);
        }
      }
    }

    await Future.wait([for (var i = 0; i < math.min(3, rows.length); i++) worker()]);
    if (_disposed) return (done, failures);
    await refresh();
    if (uncertain) {
      error = l10n.wslcCheckTheResultsOfSomeOperationsCommandsWill;
      stale = true;
    }
    if (action == 'remove' && openedContainerId != null && !containers.any((c) => c.id == openedContainerId)) {
      openedContainerId = null;
    }
    notifyListeners();
    return (done, failures);
  }

  /// Runs a removal and re-reads [target]. Returns an error text or null on success.
  Future<String?> _remove(String key, DeskSection target, Future<void> Function() action) async {
    if (!canMutate || busyIds.contains(key)) {
      return l10n.wslcActionCurrentlyUnavailable;
    }
    busyIds.add(key);
    notifyListeners();
    try {
      await action();
      return null;
    } catch (e) {
      return errorText(e);
    } finally {
      busyIds.remove(key);
      if (!_disposed) {
        await refresh();
        if (target != DeskSection.containers && target != section) {
          await _load(target).catchError((_) => false);
        }
        notifyListeners();
      }
    }
  }

  Future<String?> removeContainer(String id, {required bool force}) async {
    _touch([id]);
    final error = await _remove(id, DeskSection.containers, () => api.removeContainer(containerId: id, force: force));
    if (error == null && openedContainerId == id) closeContainer();
    return error;
  }

  Future<String?> removeImage(String id) => _remove(id, DeskSection.images, () => api.removeImage(imageId: id));

  Future<String?> removeNetwork(String name) =>
      _remove('network:$name', DeskSection.networks, () => api.removeNetwork(networkName: name));

  Future<String?> removeVolume(String name) =>
      _remove('volume:$name', DeskSection.storage, () => api.removeVolume(volumeName: name));

  @override
  void dispose() {
    _disposed = true;
    _poller?.cancel();
    _eventsTimer?.cancel();
    _debounce?.cancel();
    unawaited(_events.close());
    stats.dispose();
    notificationsEnabled.dispose();
    closeToTray.dispose();
    autostart.dispose();
    syncing.dispose();
    lastSync.dispose();
    themeMode.dispose();
    accentColor.dispose();
    backgroundTint.dispose();
    super.dispose();
  }
}

String _actionLabel(AppLocalizations l10n, String action) => switch (action) {
  'start' => l10n.wslcStart,
  'stop' => l10n.wslcStop,
  'restart' => l10n.wslcRestart,
  _ => action,
};
