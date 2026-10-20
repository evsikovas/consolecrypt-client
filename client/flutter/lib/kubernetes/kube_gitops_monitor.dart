import 'dart:async';

import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_watch_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_view_preferences_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Only an active, unlocked browser owns a monitor. No timer or subscription is
/// allowed to outlive it, and reminders never invoke cluster operations.
class KubeGitOpsMonitor extends ChangeNotifier {
  KubeGitOpsMonitor({
    required this.session,
    required this.service,
    required this.isCurrent,
    this.preferences,
    this.onNotice,
    DateTime Function()? clock,
  }) : _clock = clock ?? DateTime.now {
    unawaited(_readPreferences());
  }

  final KubeSession session;
  final KubeInsightsService service;
  final KubeViewPreferencesService? preferences;
  final bool Function() isCurrent;
  final void Function(KubeGitOpsNotice)? onNotice;
  final DateTime Function() _clock;
  KubeInsights? data;
  bool loading = false, failed = false, live = false, watchPartial = false;
  bool notifications = false, preferencesFailed = false;
  final reminders = <String, KubeGitOpsReminder>{};
  final _alerts = <String>{};
  final _observed = <String, ({bool ready, bool stalled, String? failure})>{};
  bool _disposed = false, _foreground = true, _read = false, _reload = false, _saving = false;
  int _visible = 0, _generation = 0, _watchGeneration = 0;
  int _preferencesRevision = 0;
  // _stopWatch cancels on hide, background, lock and disposal.
  // ignore: cancel_subscriptions
  StreamSubscription<KubeInsightsInvalidation>? _watch;
  Timer? _debounce, _retry, _reminderTimer;
  bool get _current => !_disposed && isCurrent();
  bool get _active => _current && _foreground && (_visible > 0 || notifications);
  int get alertCount => _alerts.length;

  Future<void> _readPreferences() async {
    if (preferences == null) return;
    final revision = _preferencesRevision;
    try {
      final result = await preferences!.readKubeViewPreferences(session.contextId);
      if (!_current || revision != _preferencesRevision) return;
      final saved = result['gitops'] as KubeJson?;
      notifications = saved?['notifications'] == true;
      for (final item in (saved?['reminders'] as List? ?? const []).take(32)) {
        if (item is! KubeJson) continue;
        final reminder = KubeGitOpsReminder.fromJson(item);
        reminders[reminder.resource.id] = reminder;
      }
      _sync();
      notifyListeners();
    } catch (_) {
      if (_current) {
        preferencesFailed = true;
        notifyListeners();
      }
    }
  }

  void show() {
    _visible++;
    _sync();
  }

  void hide() {
    _visible = (_visible - 1).clamp(0, 100);
    _sync();
  }

  void setForeground(bool value) {
    if (!_current || _foreground == value) return;
    _foreground = value;
    _sync();
  }

  void _sync() {
    if (!_current) return;
    _reminderTimer?.cancel();
    if (_foreground && reminders.isNotEmpty) {
      _reminderTimer = Timer.periodic(const Duration(seconds: 1), (_) => checkReminders());
      scheduleMicrotask(checkReminders);
    }
    if (!_active) {
      _generation++;
      loading = false;
      _stopWatch();
    } else {
      if (data == null && !loading) unawaited(refresh());
      _startWatch();
    }
  }

  void _startWatch() {
    if (!_active || _watch != null || service is! KubeInsightsWatchService) return;
    final generation = ++_watchGeneration;
    _watch = (service as KubeInsightsWatchService)
        .watchInsights(session.id)
        .listen(
          (event) {
            if (!_active || generation != _watchGeneration) return;
            live = !event.closed;
            watchPartial = event.partial;
            _debounce?.cancel();
            _debounce = Timer(const Duration(milliseconds: 800), () => unawaited(refresh()));
            notifyListeners();
          },
          onError: (Object error) => _watchEnded(generation),
          onDone: () => _watchEnded(generation),
        );
  }

  void _watchEnded(int generation) {
    if (!_current || generation != _watchGeneration) return;
    final watch = _watch;
    _watch = null;
    if (watch != null) unawaited(watch.cancel());
    live = false;
    notifyListeners();
    _retry?.cancel();
    if (_active) _retry = Timer(const Duration(seconds: 5), _startWatch);
  }

  void _stopWatch() {
    _watchGeneration++;
    _debounce?.cancel();
    _retry?.cancel();
    final watch = _watch;
    _watch = null;
    if (watch != null) unawaited(watch.cancel());
    live = false;
  }

  Future<void> refresh() async {
    if (!_active) return;
    if (_read) {
      _reload = true;
      return;
    }
    _read = true;
    final generation = _generation;
    loading = true;
    failed = false;
    notifyListeners();
    try {
      final next = await service.insights(session.id);
      if (_active && generation == _generation) {
        _observe(next);
        data = next;
      }
    } catch (_) {
      if (_active && generation == _generation) failed = true;
    } finally {
      _read = false;
      if (_current) {
        loading = false;
        notifyListeners();
        if (_reload && _active) {
          _reload = false;
          unawaited(refresh());
        }
      }
    }
  }

  void _observe(KubeInsights next) {
    final changed = <KubeResourceRef>[];
    for (final resource in next.resources.where((r) => r.isFlux)) {
      final key = '${resource.reference.id}/${resource.resource.uid}';
      final before = _observed[key];
      final ready = kubeGitOpsReady(resource);
      final failure = _failureStamp(resource);
      final state = (
        ready: ready,
        stalled: kubeGitOpsStalled(resource),
        failure: ready ? null : failure ?? before?.failure,
      );
      if (notifications &&
          before != null &&
          !resource.suspended &&
          ((before.ready && !state.ready) ||
              (!before.stalled && state.stalled) ||
              (failure != null && before.failure != null && before.failure != failure))) {
        _alerts.add(resource.reference.id);
        changed.add(resource.reference);
      }
      _observed[key] = state;
    }
    if (_observed.length > 10000) {
      final ids = next.resources.map((r) => '${r.reference.id}/${r.resource.uid}').toSet();
      _observed.removeWhere((key, _) => !ids.contains(key));
    }
    if (changed.isNotEmpty) onNotice?.call(KubeGitOpsNotice(resources: changed));
  }

  String? _failureStamp(KubeInsightResource resource) {
    final status = resource.object['status'] as KubeJson? ?? const {};
    final conditions = (status['conditions'] as List? ?? const []).whereType<KubeJson>();
    if (conditions.any((c) => c['type'] == 'Reconciling' && c['status'] == 'True')) return null;
    final ready = conditions.where((c) => c['type'] == 'Ready' && c['status'] == 'False').firstOrNull;
    final time = ready?['lastTransitionTime'] as String?;
    if (time == null || DateTime.tryParse(time) == null) return null;
    final generation = (resource.object['metadata'] as Map?)?['generation'];
    if (generation != null && ready?['observedGeneration'] != generation) return null;
    return '$time/${status['lastHandledReconcileAt'] ?? ''}';
  }

  Future<void> setNotifications(bool value) async {
    if (!_current) return;
    _preferencesRevision++;
    notifications = value;
    _observed.clear(); // Enabling establishes a new baseline, never a burst of old failures.
    _sync();
    notifyListeners();
    await _save();
  }

  Future<void> remind(KubeResourceRef resource, Duration delay) async {
    if (!_current || preferences == null || delay <= Duration.zero) return;
    if (reminders.length >= 32 && !reminders.containsKey(resource.id)) return;
    _preferencesRevision++;
    reminders[resource.id] = KubeGitOpsReminder(resource, _clock().add(delay));
    _sync();
    notifyListeners();
    await _save();
  }

  Future<void> cancelReminder(String id) async {
    if (!_current) return;
    _preferencesRevision++;
    reminders.remove(id);
    _sync();
    notifyListeners();
    await _save();
  }

  void checkReminders() {
    if (!_current || !_foreground) return;
    final due = reminders.values.where((r) => !r.dueAt.isAfter(_clock())).toList();
    if (due.isEmpty) return;
    _preferencesRevision++;
    for (final reminder in due) {
      reminders.remove(reminder.resource.id);
      _alerts.add(reminder.resource.id);
    }
    onNotice?.call(KubeGitOpsNotice(resources: due.map((r) => r.resource).toList(), reminder: true));
    _sync();
    notifyListeners();
    unawaited(_save());
  }

  void acknowledge() {
    if (!_current) return;
    _alerts.clear();
    notifyListeners();
  }

  bool _saveAgain = false;
  Future<void> _save() async {
    if (!_current || preferences == null) return;
    if (_saving) {
      _saveAgain = true;
      return;
    }
    _saving = true;
    try {
      do {
        _saveAgain = false;
        await preferences!.updateKubeViewPreferences(
          session.contextId,
          patch: {
            'kind': 'gitops',
            'preferences': {
              'notifications': notifications,
              'reminders': reminders.values.map((r) => r.toJson()).toList(),
            },
          },
        );
      } while (_current && _saveAgain);
      if (_current) preferencesFailed = false;
    } catch (_) {
      if (_current) preferencesFailed = true;
    } finally {
      _saving = false;
      if (_current) notifyListeners();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _generation++;
    _stopWatch();
    _reminderTimer?.cancel();
    data = null;
    reminders.clear();
    _observed.clear();
    _alerts.clear();
    super.dispose();
  }
}

final class KubeGitOpsReminder {
  const KubeGitOpsReminder(this.resource, this.dueAt);
  final KubeResourceRef resource;
  final DateTime dueAt;
  factory KubeGitOpsReminder.fromJson(KubeJson json) => KubeGitOpsReminder(
    KubeResourceRef.fromJson(json['resource'] as KubeJson),
    DateTime.fromMillisecondsSinceEpoch(json['due_at_ms'] as int),
  );
  KubeJson toJson() => {'resource': resource.toJson(), 'due_at_ms': dueAt.millisecondsSinceEpoch};
}

final class KubeGitOpsNotice {
  const KubeGitOpsNotice({required this.resources, this.reminder = false});
  final List<KubeResourceRef> resources;
  final bool reminder;
}

bool kubeGitOpsReady(KubeInsightResource resource) {
  final status = resource.object['status'] as KubeJson? ?? const {};
  if (resource.isArgo) {
    return (status['health'] as KubeJson?)?['status'] == 'Healthy' &&
        (status['sync'] as KubeJson?)?['status'] == 'Synced';
  }
  return (status['conditions'] as List? ?? const []).whereType<KubeJson>().any(
    (c) => c['type'] == 'Ready' && c['status'] == 'True',
  );
}

bool kubeGitOpsStalled(KubeInsightResource resource) =>
    ((resource.object['status'] as KubeJson?)?['conditions'] as List? ?? const []).whereType<KubeJson>().any(
      (c) => c['type'] == 'Stalled' && c['status'] == 'True',
    );

/// Sidebar metadata only, reset on every vault/profile boundary.
final kubeGitOpsAlertsProvider = NotifierProvider<KubeGitOpsAlerts, Map<String, int>>(KubeGitOpsAlerts.new);

final class KubeGitOpsAlerts extends Notifier<Map<String, int>> {
  @override
  Map<String, int> build() {
    ref.watch(kubeScopeProvider);
    return const {};
  }

  void update(KubeScope scope, String contextId, int count) {
    if (!ref.mounted || !scope.unlocked || !identical(scope, ref.read(kubeScopeProvider))) return;
    state = {...state, contextId: count};
  }
}
