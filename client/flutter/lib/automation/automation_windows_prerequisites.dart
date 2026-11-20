import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';

/// Optional fixed Native inspection. No command, action, elevation or installation input.
abstract interface class AutomationWindowsPrerequisiteService {
  Future<AutomationJson> inspectWindowsPrerequisites({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
  });
}

enum WindowsFeatureObservation {
  enabled,
  disabled,
  enablePending,
  disablePending,
  unknown,
}

enum WindowsFeatureUnknown { requiresElevation, unavailable }

enum WindowsElevationObservation { elevated, notElevated, unknown }

enum WindowsRebootObservation { indicatorsPresent, notObserved, unknown }

enum WindowsRebootIndicator {
  componentServicing,
  windowsUpdate,
  pendingFileRename,
}

Never _invalid() =>
    throw const FormatException('Invalid Windows prerequisite observation');
void _keys(AutomationJson value, Set<String> keys) {
  if (value.length != keys.length || !value.keys.toSet().containsAll(keys)) {
    _invalid();
  }
}

AutomationJson _map(Object? value) =>
    value is Map<String, dynamic> ? value : _invalid();

final class WindowsFeatureFact {
  const WindowsFeatureFact(this.state, this.reason);
  final WindowsFeatureObservation state;
  final WindowsFeatureUnknown? reason;
  factory WindowsFeatureFact.parse(Object? raw) {
    final value = _map(raw);
    _keys(value, {'state', 'reason'});
    final state = WindowsFeatureObservation.values.byName(
      value['state'] as String,
    );
    final reason = value['reason'] == null
        ? null
        : WindowsFeatureUnknown.values.byName(value['reason'] as String);
    if ((state == WindowsFeatureObservation.unknown) != (reason != null)) {
      _invalid();
    }
    return WindowsFeatureFact(state, reason);
  }
}

/// Original Native facts only. No combined Ready/repair permission is derived.
final class WindowsPrerequisiteSnapshot {
  const WindowsPrerequisiteSnapshot(
    this.capturedAt,
    this.elevation,
    this.subsystem,
    this.virtualMachine,
    this.reboot,
    this.indicators,
  );
  final DateTime capturedAt;
  final WindowsElevationObservation elevation;
  final WindowsFeatureFact subsystem, virtualMachine;
  final WindowsRebootObservation reboot;
  final List<WindowsRebootIndicator> indicators;
  factory WindowsPrerequisiteSnapshot.parse(
    AutomationJson value,
    RuntimeNoticeScope original,
  ) {
    if (utf8.encode(jsonEncode(value)).length > 16384) _invalid();
    _keys(value, {
      'version',
      'profileId',
      'notificationScopeId',
      'capturedAtUtc',
      'source',
      'elevation',
      'features',
      'reboot',
    });
    if (value['version'] is! int ||
        value['version'] != 1 ||
        value['source'] != 'windowsReadOnlyV1' ||
        value['profileId'] != original.profileId ||
        value['notificationScopeId'] != original.nonce) {
      _invalid();
    }
    final time = value['capturedAtUtc'];
    if (time is! String ||
        time.length > 40 ||
        !RegExp(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?Z$')
            .hasMatch(time)) {
      _invalid();
    }
    final captured = DateTime.tryParse(time);
    if (captured == null ||
        !captured.isUtc ||
        captured.toIso8601String().substring(0, 19) != time.substring(0, 19)) {
      _invalid();
    }
    final elevation = _map(value['elevation']);
    _keys(elevation, {'state', 'reason'});
    final privilege = WindowsElevationObservation.values.byName(
      elevation['state'] as String,
    );
    if (privilege == WindowsElevationObservation.unknown
        ? elevation['reason'] != 'unavailable'
        : elevation['reason'] != null) {
      _invalid();
    }
    final features = _map(value['features']);
    _keys(features, {'subsystemLinux', 'virtualMachinePlatform'});
    final reboot = _map(value['reboot']);
    _keys(reboot, {'state', 'reason', 'indicators'});
    final restart = WindowsRebootObservation.values.byName(
          reboot['state'] as String,
        ),
        rawIndicators = reboot['indicators'];
    if (rawIndicators is! List ||
        rawIndicators.length > 3 ||
        rawIndicators.toSet().length != rawIndicators.length) {
      _invalid();
    }
    final indicators = rawIndicators
        .map((v) => WindowsRebootIndicator.values.byName(v as String))
        .toList(growable: false);
    if (restart == WindowsRebootObservation.indicatorsPresent
        ? indicators.isEmpty || reboot['reason'] != null
        : restart == WindowsRebootObservation.notObserved
        ? indicators.isNotEmpty || reboot['reason'] != null
        : indicators.isNotEmpty || reboot['reason'] != 'unavailable') {
      _invalid();
    }
    final subsystem = WindowsFeatureFact.parse(features['subsystemLinux']),
        machine = WindowsFeatureFact.parse(features['virtualMachinePlatform']);
    for (final feature in [subsystem, machine]) {
      if (feature.state == WindowsFeatureObservation.unknown) {
        if (feature.reason !=
            (privilege == WindowsElevationObservation.notElevated
                ? WindowsFeatureUnknown.requiresElevation
                : WindowsFeatureUnknown.unavailable)) {
          _invalid();
        }
      } else if (privilege != WindowsElevationObservation.elevated) {
        _invalid();
      }
    }
    return WindowsPrerequisiteSnapshot(
      captured,
      privilege,
      subsystem,
      machine,
      restart,
      List.unmodifiable(indicators),
    );
  }
}
