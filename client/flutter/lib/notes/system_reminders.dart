import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:timezone/data/latest.dart' as tz_data;
import 'package:timezone/timezone.dart' as tz;

/// Most reminders handed to the system at once (the nearest ones).
const maxSystemReminders = 64;

/// System notifications for note reminders (FEAT-07), so they appear while
/// the app is closed. The notification never carries note content: the
/// system notification centre is not encrypted.
abstract interface class ReminderScheduler {
  /// Replaces every scheduled reminder with one notification at each of [at]
  /// showing [title] and [body].
  Future<void> replace(List<DateTime> at, {required String title, required String body});

  /// Cancels every scheduled reminder (profile signed out or deleted).
  Future<void> clear();
}

/// The reminders the system should hold: future ones, nearest first, at
/// most [maxSystemReminders], each instant once.
List<DateTime> systemReminderTimes(Iterable<DateTime?> remindAt, DateTime now) {
  final times = remindAt.whereType<DateTime>().where((at) => at.isAfter(now)).map((at) => at.toUtc()).toSet().toList()
    ..sort();
  return times.take(maxSystemReminders).toList();
}

/// `flutter_local_notifications`: zoned schedules in UTC. Linux cannot
/// schedule, so reminders there show only while the app runs.
final class LocalNotificationScheduler implements ReminderScheduler {
  final _plugin = FlutterLocalNotificationsPlugin();
  Future<bool>? _ready;
  static const _firstId = 7000;

  bool get _supported => !kIsWeb && !AppPlatform.isLinux;

  Future<bool> _initialize() async {
    tz_data.initializeTimeZones();
    const darwin = DarwinInitializationSettings(
      requestAlertPermission: false,
      requestBadgePermission: false,
      requestSoundPermission: false,
    );
    final ok = await _plugin.initialize(
      settings: const InitializationSettings(
        android: AndroidInitializationSettings('@mipmap/ic_launcher'),
        iOS: darwin,
        macOS: darwin,
        windows: WindowsInitializationSettings(
          appName: 'ConsoleCrypt',
          appUserModelId: 'ConsoleCrypt.Desktop',
          guid: '6f1d6f1a-3c3b-4f7e-9a8e-5b9c0d2e4a71',
        ),
      ),
    );
    return ok ?? false;
  }

  /// Asks for permission once, when there is something to schedule.
  Future<void> _permission() async {
    await _plugin
        .resolvePlatformSpecificImplementation<AndroidFlutterLocalNotificationsPlugin>()
        ?.requestNotificationsPermission();
    await _plugin.resolvePlatformSpecificImplementation<IOSFlutterLocalNotificationsPlugin>()?.requestPermissions(
      alert: true,
      sound: true,
    );
    await _plugin.resolvePlatformSpecificImplementation<MacOSFlutterLocalNotificationsPlugin>()?.requestPermissions(
      alert: true,
      sound: true,
    );
  }

  @override
  Future<void> replace(List<DateTime> at, {required String title, required String body}) async {
    if (!_supported || !await (_ready ??= _initialize())) return;
    await clear();
    if (at.isEmpty) return;
    await _permission();
    const details = NotificationDetails(
      android: AndroidNotificationDetails(
        'reminders',
        'Reminders',
        importance: Importance.high,
        priority: Priority.high,
        visibility: NotificationVisibility.private,
      ),
      iOS: DarwinNotificationDetails(),
      macOS: DarwinNotificationDetails(),
    );
    for (final (index, time) in at.take(maxSystemReminders).indexed) {
      await _plugin.zonedSchedule(
        id: _firstId + index,
        title: title,
        body: body,
        scheduledDate: tz.TZDateTime.from(time.toUtc(), tz.UTC),
        notificationDetails: details,
        // Inexact: no exact-alarm permission; a reminder may come a little late.
        androidScheduleMode: AndroidScheduleMode.inexactAllowWhileIdle,
      );
    }
  }

  @override
  Future<void> clear() async {
    if (!_supported || !await (_ready ??= _initialize())) return;
    for (var id = _firstId; id < _firstId + maxSystemReminders; id++) {
      await _plugin.cancel(id: id);
    }
  }
}

final reminderSchedulerProvider = Provider<ReminderScheduler>((ref) => LocalNotificationScheduler());
