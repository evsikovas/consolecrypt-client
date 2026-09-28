import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

/// A zero-latency, seeded mock backend (no timers left pending).
MockBackend testBackend({bool seed = true}) => MockBackend(config: MockConfig.test(seedDemoData: seed));

/// Stores the UI language on [backend] (device-local setting; the mock
/// applies it synchronously).
void setTestLocale(MockBackend backend, AppLocale locale) {
  final settings = backend.services.settings;
  // The mock stores the value synchronously; the returned future is already complete.
  settings.updateLocal(settings.currentLocal.copyWith(appLocale: locale)).ignore();
}

/// Pumps the full app on [backend] in a desktop-sized window, in [locale]
/// (English by default so tests do not depend on the host OS language).
Future<void> pumpApp(
  WidgetTester tester,
  MockBackend backend, {
  AppLocale locale = AppLocale.en,
  Size size = const Size(1600, 1000),
}) async {
  setTestLocale(backend, locale);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [appServicesProvider.overrideWithValue(backend.services)],
      retry: (_, _) => null,
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
}

/// Advances fake time in small steps (dialogs/route transitions finish,
/// microtask-based mock work completes). Unlike `pumpAndSettle` this also
/// terminates when an indeterminate progress indicator is on screen.
Future<void> settle(WidgetTester tester, {int steps = 12}) async {
  for (var i = 0; i < steps; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<void> tapKey(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.pump();
  await tester.tap(finder);
  await settle(tester);
}

Future<void> enterKey(WidgetTester tester, String key, String text) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.enterText(finder, text);
  await tester.pump();
}

/// Picks [hostName] in the host picker dialog (the list is virtualised).
Future<void> pickHost(WidgetTester tester, String hostName) async {
  await tester.enterText(find.byKey(const ValueKey('host-picker-search')), hostName);
  await tester.pump();
  await tester.tap(find.byKey(ValueKey('pick-$hostName')));
  await settle(tester);
}

/// Whether the button with [key] is enabled (kit or material_ui buttons).
bool isEnabled(WidgetTester tester, String key) {
  final widget = tester.widget(find.byKey(ValueKey(key)));
  return switch (widget) {
    GlassButton(:final onPressed, :final busy) => onPressed != null && !busy,
    GlassIconButton(:final onPressed) => onPressed != null,
    ButtonStyleButton(:final onPressed) => onPressed != null,
    _ => throw StateError('not a button: ${widget.runtimeType}'),
  };
}

/// The open dialog(s): kit dialogs ([GlassDialog]) and any remaining
/// material_ui alert dialogs.
Finder findDialog() => find.byWidgetPredicate((w) => w is GlassDialog || w is AlertDialog);
