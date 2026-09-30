// Simulator regression: real Rust/SSH/SFTP, generated loopback-only fixture.
// Flutter TextInput events emulate IME commits; physical keyboard, biometric
// authentication and OS background suspension require a real-device test.
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:xterm/xterm.dart';

import 'ios_core_test.dart' as core_tests;

const _controlPort = int.fromEnvironment('CC_IOS_FIXTURE_CONTROL_PORT');

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  // One native compilation runs both the storage and the SSH suites.
  core_tests.main();
  testWidgets(
    'iOS Cyrillic IME, selection/paste, tabs, reconnect and SFTP',
    (tester) async {
      expect(_controlPort, greaterThan(0), reason: 'Run client/scripts/ci-ios.sh --test-only.');
      final fixture = await _Fixture.load();
      final privateRoot = await const MethodChannel('consolecrypt/ios').invokeMethod<String>('dataDirectory');
      final root = await Directory(privateRoot!).createTemp('ios-ssh-it-');
      final rust = await RustAppServices.open(
        RustCoreOptions(
          dataDir: '${root.path}/data',
          keychainService: 'io.consolecrypt.ios.ssh-it-${DateTime.now().microsecondsSinceEpoch}',
          fastKdfForTests: true,
          backgroundSync: false,
          autoStartTunnels: false,
          hookAppExit: false,
        ),
      );
      final services = rust.services;
      final priorClipboard = await Clipboard.getData(Clipboard.kTextPlain);
      ProviderContainer? container;
      try {
        await services.profiles.createLocalProfile(name: 'iOS SSH regression');
        await services.vault.createVault(
          name: 'Generated test vault',
          passphrase: SecretText('runtime-${ObjectId.generate().value}-iOS'),
        );
        await services.vault.confirmRecoveryKitSaved();
        await services.settings.updateLocal(services.settings.currentLocal.copyWith(appLocale: AppLocale.en));
        final now = DateTime.now().toUtc();
        final host = await services.inventory.saveHostWithAuth(
          Host(
            id: ObjectId.generate(),
            name: 'Loopback SSH fixture',
            address: '127.0.0.1',
            port: fixture.sshPort,
            username: fixture.username,
            // Docker's forwarded TCP connection can outlive a stopped server.
            // Keep this generated fixture's failure detection deterministic.
            keepaliveSecs: 2,
            createdAt: now,
            updatedAt: now,
          ),
          HostAuthInlinePassword(password: SecretText(fixture.password)),
        );
        tester.testTextInput.register();
        await tester.pumpWidget(
          ProviderScope(
            overrides: [appServicesProvider.overrideWithValue(services)],
            retry: (_, _) => null,
            child: const ConsoleCryptApp(),
          ),
        );
        await _until(tester, () => find.byType(Navigator).evaluate().isNotEmpty, 'App navigator');
        container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
        await _until(tester, () => container!.read(appStageProvider) == AppStage.unlocked, 'Unlocked app');
        container.read(routerProvider).go(AppRoutes.terminal);
        await tester.pump();
        final tabs = container.read(terminalTabsProvider.notifier);
        await tabs.open(host);
        final first = container.read(terminalTabsProvider).active!;
        await _until(tester, () => first.hostKey != null, 'SSH host key');
        // Compare against the key read directly from this generated container.
        // Changed or unexpected keys are never blindly accepted by the test.
        expect(first.hostKey!.changed, isFalse);
        expect(first.hostKey!.fingerprintSha256, fixture.fingerprint);
        await _tap(tester, 'accept-host-key');
        await _until(tester, () => first.isConnected, 'SSH connected');

        await _typeCommand(tester, "printf 'CC_IME_%s\\n' 'Привет, мир'", composing: true);
        await _until(tester, () => _output(first).contains('CC_IME_Привет, мир'), 'Cyrillic SSH output');

        // iOS Return in a newline-configured TextInput client can arrive as
        // committed text. Exercise it as well as the done-action path below.
        await tester.tap(find.byType(TerminalView));
        await tester.pump();
        tester.testTextInput.enterText("printf 'CC_RETURN_%s\\n' 'Ввод'\n");
        await _until(tester, () => _output(first).contains('CC_RETURN_Ввод'), 'Committed Return input');

        await _tap(tester, 'terminal-context-menu');
        await _tap(tester, 'terminal-menu-select-all');
        expect(first.selectedText, contains('CC_IME_Привет, мир'));
        await _tap(tester, 'terminal-context-menu');
        await _tap(tester, 'terminal-menu-copy');
        expect((await Clipboard.getData(Clipboard.kTextPlain))?.text, contains('CC_IME_Привет, мир'));

        await Clipboard.setData(const ClipboardData(text: "printf 'CC_PASTE_%s\\n' 'Вставка'"));
        await _tap(tester, 'terminal-context-menu');
        await _tap(tester, 'terminal-menu-paste');
        await tester.pump(const Duration(milliseconds: 150));
        expect(_output(first), isNot(contains('CC_PASTE_Вставка')));
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await _until(tester, () => _output(first).contains('CC_PASTE_Вставка'), 'Pasted command output');

        await tabs.open(host);
        final second = container.read(terminalTabsProvider).active!;
        await _until(tester, () => second.isConnected, 'Second SSH tab');
        await _typeCommand(tester, "printf 'CC_TAB_%s\\n' 'Вторая'");
        await _until(tester, () => _output(second).contains('CC_TAB_Вторая'), 'Active tab input');
        expect(_output(first), isNot(contains('CC_TAB_Вторая')));
        await _tap(tester, 'tab-Loopback SSH fixture-0');
        await _typeCommand(tester, "printf 'CC_TAB_%s\\n' 'Первая'");
        await _until(tester, () => _output(first).contains('CC_TAB_Первая'), 'First tab reactivation');

        // Flutter lifecycle injection proves one-tap IME reattachment only.
        // It does not emulate iOS process suspension or a device locking.
        binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
        binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
        binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
        await tester.pump();
        FocusManager.instance.primaryFocus?.unfocus();
        await _typeCommand(tester, "printf 'CC_RESUME_%s\\n' 'Возврат'");
        await _until(tester, () => _output(first).contains('CC_RESUME_Возврат'), 'Input after foreground return');

        debugPrint('iOS regression: SSH keyboard, selection, paste and tab checks completed.');
        final sftp = await _io(tester, services.sftp.connect(host.id), 'SFTP connect');
        try {
          final home = await _io(tester, services.sftp.remoteHome(sftp), 'SFTP remote home');
          final name = 'проверка-${DateTime.now().microsecondsSinceEpoch}.txt';
          final source = await File('${root.path}/$name').writeAsString('Кириллица и UTF-8: Привет, мир\n');
          final upload = await _io(
            tester,
            services.sftp.upload(sftp, localPath: source.path, remoteDirectory: home),
            'SFTP upload queued',
          );
          await _transfer(tester, services.sftp.watchTransfers(), upload);
          expect((await services.sftp.listRemote(sftp, home)).map((entry) => entry.name), contains(name));
          final destination = await Directory('${root.path}/download').create();
          final download = await _io(
            tester,
            services.sftp.download(sftp, remotePath: joinRemotePath(home, name), localDirectory: destination.path),
            'SFTP download queued',
          );
          await _transfer(tester, services.sftp.watchTransfers(), download);
          expect(await File('${destination.path}/$name').readAsBytes(), await source.readAsBytes());
          await services.sftp.deleteRemote(sftp, joinRemotePath(home, name));
        } finally {
          await services.sftp.disconnect(sftp);
        }
        debugPrint('iOS regression: SFTP UTF-8 roundtrip completed.');

        await _io(tester, fixture.command('stop'), 'SSH fixture stopped');
        await _until(tester, () => first.state == SessionConnectionState.disconnected, 'Disconnected SSH');
        await _io(tester, fixture.command('start'), 'SSH fixture restarted');
        await _tap(tester, 'reconnect');
        await _until(tester, () => first.isConnected, 'Reconnected SSH');
        await _typeCommand(tester, "printf 'CC_RECONNECT_%s\\n' 'Снова'");
        await _until(tester, () => _output(first).contains('CC_RECONNECT_Снова'), 'Input after reconnect');
        expect(_output(first), contains('CC_IME_Привет, мир'));
        final reopened = await _io(tester, services.sftp.connect(host.id), 'SFTP reconnected');
        try {
          final home = await _io(tester, services.sftp.remoteHome(reopened), 'Reconnected SFTP home');
          expect(await _io(tester, services.sftp.listRemote(reopened, home), 'Reconnected SFTP listing'), isNotEmpty);
        } finally {
          await services.sftp.disconnect(reopened);
        }
        // Closing an idle live bridge stream must finish without waiting for
        // another remote output frame. This assertion is separate from the
        // bounded best-effort teardown of an unreachable generated fixture.
        expect(first.state, SessionConnectionState.connected);
        await _io(
          tester,
          tabs.close(first),
          'Close reconnected active SSH tab',
          timeout: const Duration(seconds: 5),
        );
        expect(
          container.read(terminalTabsProvider).tabs.any((tab) => tab.sessionId == first.sessionId),
          isFalse,
        );
        expect(tester.takeException(), isNull);
      } finally {
        if (container != null) {
          for (final tab in container.read(terminalTabsProvider).tabs.toList()) {
            await _cleanup(tester, container.read(terminalTabsProvider.notifier).close(tab), 'Close test SSH tab');
          }
        }
        await tester.pumpWidget(const SizedBox.shrink());
        tester.testTextInput.unregister();
        await Clipboard.setData(ClipboardData(text: priorClipboard?.text ?? ''));
        for (final profile in services.profiles.currentProfiles.profiles) {
          await _cleanup(tester, services.profiles.delete(profile.id), 'Delete test profile');
        }
        await _cleanup(tester, rust.close(), 'Close test core');
        await root.delete(recursive: true);
      }
    },
    skip: !Platform.isIOS,
    timeout: const Timeout(Duration(minutes: 5)),
  );
}

String _output(TerminalTab tab) => tab.terminal.buffer.getText();

Future<void> _until(
  WidgetTester tester,
  bool Function() condition,
  String label, {
  Duration timeout = const Duration(seconds: 30),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (!condition()) {
    if (DateTime.now().isAfter(deadline)) {
      debugPrint('iOS regression: Timed out: $label');
      fail('Timed out: $label');
    }
    await tester.pump(const Duration(milliseconds: 100));
  }
  await tester.pump();
}

Future<void> _tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await _until(tester, () => finder.evaluate().isNotEmpty, key);
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await tester.pump(const Duration(milliseconds: 200));
}

Future<T> _io<T>(
  WidgetTester tester,
  Future<T> future,
  String label, {
  Duration timeout = const Duration(seconds: 30),
}) async {
  late T result;
  Object? failure;
  StackTrace? stack;
  var completed = false;
  future
      .then<void>(
        (value) {
          result = value;
          completed = true;
        },
        onError: (Object error, StackTrace trace) {
          failure = error;
          stack = trace;
          completed = true;
        },
      )
      .ignore();
  await _until(tester, () => completed, label, timeout: timeout);
  if (failure != null) Error.throwWithStackTrace(failure!, stack!);
  debugPrint('iOS regression: $label completed.');
  return result;
}

Future<void> _cleanup(WidgetTester tester, Future<void> future, String label) async {
  try {
    await _io(tester, future, label, timeout: const Duration(seconds: 5));
  } catch (_) {
    // An unreachable test SSH channel must not hide the original assertion.
    // ci-ios.sh always deletes this test's dedicated Simulator and container.
    debugPrint('iOS regression: $label unfinished; ephemeral Simulator will be deleted.');
  }
}

Future<void> _typeCommand(WidgetTester tester, String text, {bool composing = false}) async {
  await tester.tap(find.byType(TerminalView));
  await tester.pump();
  expect(tester.testTextInput.hasAnyClients, isTrue, reason: 'A single touch must attach terminal input.');
  if (composing) {
    tester.testTextInput.updateEditingValue(
      TextEditingValue(
        text: text,
        selection: TextSelection.collapsed(offset: text.length),
        composing: TextRange(start: 0, end: text.length),
      ),
    );
    await tester.pump();
  }
  tester.testTextInput.enterText(text);
  await tester.pump();
  await tester.testTextInput.receiveAction(TextInputAction.done);
}

Future<void> _transfer(WidgetTester tester, Stream<List<TransferJob>> stream, TransferId id) async {
  TransferJob? current;
  final subscription = stream.listen((jobs) {
    for (final job in jobs) {
      if (job.id == id) current = job;
    }
  });
  try {
    await _until(tester, () => current != null && !current!.isActive, 'SFTP transfer');
    expect(current!.state, TransferState.completed, reason: current!.error);
  } finally {
    await subscription.cancel();
  }
}

final class _Fixture {
  const _Fixture(this.sshPort, this.username, this.password, this.fingerprint);
  final int sshPort;
  final String username;
  final String password; // Generated test credential; never printed or persisted.
  final String fingerprint;

  static Future<_Fixture> load() async {
    final config = jsonDecode(await _request('config')) as Map<String, dynamic>;
    return _Fixture(
      config['port'] as int,
      config['username'] as String,
      config['password'] as String,
      config['fingerprint'] as String,
    );
  }

  Future<void> command(String action) async => _request(action, post: true);

  static Future<String> _request(String path, {bool post = false}) async {
    final http = HttpClient();
    try {
      final uri = Uri.http('127.0.0.1:$_controlPort', '/$path');
      final request = await (post ? http.postUrl(uri) : http.getUrl(uri));
      final response = await request.close();
      if (response.statusCode != (post ? 204 : 200)) throw StateError('Test fixture control failed.');
      return await utf8.decoder.bind(response).join();
    } finally {
      http.close(force: true);
    }
  }
}
