// The owner's scenario through the real UI on the real core: create a local
// profile + vault in the app, "restart" (tear the UI down, shut the core down,
// start both again) → the app shows Vault Unlock (not onboarding) and the
// data is intact. Isolated data dir + keychain service.
//
//   flutter test integration_test/app_persistence_test.dart -d macos
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:flutter/foundation.dart' show debugPrintSynchronously;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';

const _memoryStore = bool.fromEnvironment('CC_IT_MEMORY_STORE');

final _passphrase = 'ui-${DateTime.now().microsecondsSinceEpoch}-Violet-Anchor-Muffin-Glacier';

// Diagnostics are a closed vocabulary. Never interpolate a Finder, exception,
// field value, profile, recovery word or backend response into test output.
enum _UiStep {
  setup,
  openCore,
  renderApp,
  welcomeLocal,
  newPassphrase,
  confirmPassphrase,
  createVault,
  revealKit,
  kitSaved,
  kitContinue,
  verifyReady,
  verifyWord,
  verifySubmit,
  localNotice,
  hostsReady,
  saveHost,
  hostVisible,
  reopenPreference,
  closeUi,
  closeCore,
  reopenCore,
  rerenderApp,
  unlockReady,
  unlockPassphrase,
  unlockSubmit,
  restoredHost,
  complete,
}

enum _UiEvent { begin, found, tap, input, timeout, failed, flutterError, layoutOverflow, complete }

enum _UiFailure { timeout, finderCount, notHitTestable, buttonDisabled, fieldInput, kitMissing, scenario }

_UiStep _step = _UiStep.setup;
_UiFailure? _failure;

void _diagnostic(WidgetTester tester, _UiEvent event, {Finder? finder, bool? fieldMatches, bool? buttonEnabled}) {
  var stage = 'unavailable';
  var route = 'unavailable';
  try {
    final app = find.byType(ConsoleCryptApp);
    if (app.evaluate().length == 1) {
      final container = ProviderScope.containerOf(tester.element(app));
      stage = container.read(appStageProvider).name;
      // Report a fixed route code, never the raw URI (which can contain IDs).
      route = switch (container.read(routerProvider).routeInformationProvider.value.uri.path) {
        AppRoutes.loading => 'loading',
        AppRoutes.welcome => 'welcome',
        AppRoutes.onboarding => 'onboarding',
        AppRoutes.recoveryKit => 'recoveryKit',
        AppRoutes.verifyKit => 'verifyKit',
        AppRoutes.localNotice => 'localNotice',
        AppRoutes.unlock => 'unlock',
        AppRoutes.hosts => 'hosts',
        _ => 'other',
      };
    }
  } catch (_) {
    // State sampling is best-effort; diagnostics must not expose exceptions.
  }
  debugPrintSynchronously(
    'CC_UI_DIAG ${jsonEncode({'event': event.name, 'step': _step.name, 'app_stage': stage, 'route': route, 'finder_hit': finder?.evaluate().isNotEmpty, 'hit_testable': finder?.hitTestable().evaluate().isNotEmpty, 'field_matches': fieldMatches, 'button_enabled': buttonEnabled, 'failure': _failure?.name})}',
  );
}

Never _fail(WidgetTester tester, _UiFailure failure, {Finder? finder}) {
  _failure = failure;
  _diagnostic(tester, _UiEvent.failed, finder: finder);
  throw TestFailure('CC_UI_FAILURE ${failure.name} step=${_step.name}');
}

void _begin(WidgetTester tester, _UiStep step) {
  _step = step;
  _diagnostic(tester, _UiEvent.begin);
}

/// Pumps (real time) until [finder] matches or [timeout] passes.
Future<void> _pumpUntil(
  WidgetTester tester,
  Finder finder, {
  required _UiStep step,
  Duration timeout = const Duration(seconds: 30),
}) async {
  _begin(tester, step);
  final end = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(end)) {
    await tester.pump(const Duration(milliseconds: 100));
    if (finder.evaluate().isNotEmpty) {
      _diagnostic(tester, _UiEvent.found, finder: finder);
      return;
    }
  }
  _diagnostic(tester, _UiEvent.timeout, finder: finder);
  _fail(tester, _UiFailure.timeout, finder: finder);
}

void _checkTap(WidgetTester tester, Finder finder) {
  if (finder.evaluate().length != 1) _fail(tester, _UiFailure.finderCount, finder: finder);
  final widget = tester.widget(finder);
  final enabled = switch (widget) {
    GlassButton() => widget.onPressed != null && !widget.busy,
    CheckboxListTile() => widget.onChanged != null && (widget.enabled ?? true),
    _ => null,
  };
  _diagnostic(tester, _UiEvent.tap, finder: finder, buttonEnabled: enabled);
  if (enabled == false) _fail(tester, _UiFailure.buttonDisabled, finder: finder);
  if (finder.hitTestable().evaluate().isEmpty) _fail(tester, _UiFailure.notHitTestable, finder: finder);
}

Future<void> _tapKey(WidgetTester tester, String key, _UiStep step) async {
  final f = find.byKey(ValueKey(key));
  await _pumpUntil(tester, f, step: step);
  await tester.ensureVisible(f);
  await tester.pump();
  _checkTap(tester, f);
  await tester.tap(f);
  await tester.pump(const Duration(milliseconds: 200));
}

Future<void> _enterKey(WidgetTester tester, String key, String text, _UiStep step) async {
  final f = find.byKey(ValueKey(key));
  await _pumpUntil(tester, f, step: step);
  await tester.enterText(f, text);
  await tester.pump();
  final editable = find.descendant(of: f, matching: find.byType(EditableText));
  if (editable.evaluate().length != 1) _fail(tester, _UiFailure.finderCount, finder: f);
  final matches = tester.widget<EditableText>(editable).controller.text == text;
  _diagnostic(tester, _UiEvent.input, finder: f, fieldMatches: matches);
  if (!matches) _fail(tester, _UiFailure.fieldInput, finder: f);
}

late Directory _root;
late RustCoreOptions _options;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() {
    _root = Directory.systemTemp.createTempSync('cc-bridge-ui-it-');
    _options = RustCoreOptions(
      dataDir: '${_root.path}/data',
      keychainService: 'io.consolecrypt.ConsoleCrypt.ui-it-${DateTime.now().millisecondsSinceEpoch}',
      inMemorySecureStore: _memoryStore,
      fastKdfForTests: true,
      backgroundSync: false,
      autoStartTunnels: false,
      hookAppExit: false,
    );
  });

  tearDownAll(() async {
    final rust = RustAppServices.current ?? await RustAppServices.open(_options);
    for (final p in rust.services.profiles.currentProfiles.profiles) {
      await rust.services.profiles.delete(p.id);
    }
    await rust.close();
    _root.deleteSync(recursive: true);
  });

  testWidgets('create a local vault in the app, restart → Vault Unlock, data intact', (tester) async {
    _step = _UiStep.setup;
    _failure = null;
    // Layout overflows of the shell chrome while the real window animates are
    // UI-layer issues, not what this test checks: record, don't fail.
    var hadOverflow = false;
    final onError = FlutterError.onError;
    FlutterError.onError = (details) {
      if (details.exceptionAsString().contains('overflowed')) {
        hadOverflow = true;
        _diagnostic(tester, _UiEvent.layoutOverflow);
        return;
      }
      _diagnostic(tester, _UiEvent.flutterError);
      // Preserve failure semantics while stripping error/widget payloads that
      // could include an obscured field or runtime-generated recovery word.
      final redacted = FlutterErrorDetails(
        exception: TestFailure('CC_UI_FAILURE flutterError step=${_step.name}'),
        library: 'ConsoleCrypt integration fixture',
      );
      if (onError != null) {
        onError(redacted);
      } else {
        FlutterError.dumpErrorToConsole(redacted);
      }
    };
    try {
      await _scenario(tester);
    } catch (_) {
      _failure ??= _UiFailure.scenario;
      _diagnostic(tester, _UiEvent.failed);
      throw TestFailure('CC_UI_FAILURE ${_failure!.name} step=${_step.name}');
    } finally {
      FlutterError.onError = onError;
    }
    if (hadOverflow) _diagnostic(tester, _UiEvent.layoutOverflow);
  });
}

Future<void> _scenario(WidgetTester tester) async {
  await tester.binding.setSurfaceSize(const Size(1400, 900));
  _begin(tester, _UiStep.openCore);
  var rust = await RustAppServices.open(_options);
  _begin(tester, _UiStep.renderApp);
  await tester.pumpWidget(const ProviderScope(child: ConsoleCryptApp()));

  // Welcome → "Use locally" → passphrase → Recovery Kit → 3 words → notice.
  final local = find.byKey(const ValueKey('welcome-local'));
  await _pumpUntil(tester, local, step: _UiStep.welcomeLocal);
  _checkTap(tester, local);
  await tester.tap(local);
  await _enterKey(tester, 'new-passphrase', _passphrase, _UiStep.newPassphrase);
  await _enterKey(tester, 'confirm-passphrase', _passphrase, _UiStep.confirmPassphrase);
  await _tapKey(tester, 'create-vault', _UiStep.createVault);
  await _tapKey(tester, 'reveal-kit', _UiStep.revealKit);
  await _tapKey(tester, 'kit-saved', _UiStep.kitSaved);
  await _tapKey(tester, 'kit-continue', _UiStep.kitContinue);
  await _pumpUntil(tester, find.byKey(const ValueKey('verify-submit')), step: _UiStep.verifyReady);
  final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
  final kit = container.read(onboardingControllerProvider).kit;
  if (kit == null) _fail(tester, _UiFailure.kitMissing);
  final words = kit.exposeWords();
  for (var i = 0; i < RecoveryKit.wordCount; i++) {
    final field = find.byKey(ValueKey('verify-word-$i'));
    if (field.evaluate().isNotEmpty) await _enterKey(tester, 'verify-word-$i', words[i], _UiStep.verifyWord);
  }
  await tester.pump();
  await _tapKey(tester, 'verify-submit', _UiStep.verifySubmit);
  await _tapKey(tester, 'local-notice-continue', _UiStep.localNotice);
  await _pumpUntil(tester, find.byKey(const ValueKey('nav-hosts')), step: _UiStep.hostsReady);
  expect(rust.services.vault.currentStatus.phase, VaultPhase.unlocked);
  expect(rust.services.vault.currentStatus.recoveryKitConfirmed, isTrue);

  _begin(tester, _UiStep.saveHost);
  await rust.services.inventory.saveHostWithAuth(
    Host.create(name: 'persisted-host', address: '192.0.2.10'),
    HostAuthInlinePassword(password: SecretText('pw-${DateTime.now().microsecondsSinceEpoch}')),
  );
  await _pumpUntil(tester, find.text('persisted-host'), step: _UiStep.hostVisible);

  if (_memoryStore) {
    _begin(tester, _UiStep.complete);
    _diagnostic(tester, _UiEvent.complete);
    return; // keys do not survive a restart then
  }

  // Reopening the last profile is opt-in; exercise that explicit preference.
  _begin(tester, _UiStep.reopenPreference);
  await rust.services.settings.updateLocal(rust.services.settings.currentLocal.copyWith(reopenLastProfile: true));

  // Restart: UI gone, core shut down (SQLCipher closed), then both again.
  _begin(tester, _UiStep.closeUi);
  await tester.pumpWidget(const SizedBox.shrink());
  _begin(tester, _UiStep.closeCore);
  await rust.close();
  _begin(tester, _UiStep.reopenCore);
  rust = await RustAppServices.open(_options);
  _begin(tester, _UiStep.rerenderApp);
  await tester.pumpWidget(const ProviderScope(child: ConsoleCryptApp()));

  await _pumpUntil(tester, find.byKey(const ValueKey('unlock-passphrase')), step: _UiStep.unlockReady);
  expect(find.byKey(const ValueKey('new-passphrase')), findsNothing, reason: 'not onboarding');
  expect(find.byKey(const ValueKey('option-local')), findsNothing, reason: 'not the welcome screen');
  await _enterKey(tester, 'unlock-passphrase', _passphrase, _UiStep.unlockPassphrase);
  await _tapKey(tester, 'unlock-submit', _UiStep.unlockSubmit);
  await _pumpUntil(tester, find.text('persisted-host'), step: _UiStep.restoredHost);
  expect(rust.services.vault.currentStatus.phase, VaultPhase.unlocked);
  _begin(tester, _UiStep.closeUi);
  await tester.pumpWidget(const SizedBox.shrink());
  _begin(tester, _UiStep.complete);
  _diagnostic(tester, _UiEvent.complete);
}
