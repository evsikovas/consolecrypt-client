import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('choose, cancel, replace and reset the device default editor', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    final settings = b.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(uiFontScale: 1.4));
    await pumpApp(tester, b, locale: AppLocale.ru, size: const Size(1024, 720));
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'sftp-default-editor-choose');
    const code = AppRef(AppRefKind.path, '/Applications/Visual Studio Code.app');
    expect(settings.currentLocal.sftpDefaultEditor, code);
    expect(find.text('Visual Studio Code'), findsOneWidget);
    b.files.applicationPath = null;
    await tapKey(tester, 'sftp-default-editor-choose');
    expect(settings.currentLocal.sftpDefaultEditor, code);
    b.files.applicationPath = '/Applications/Zed.app';
    await tapKey(tester, 'sftp-default-editor-choose');
    expect(settings.currentLocal.sftpDefaultEditor, const AppRef(AppRefKind.path, '/Applications/Zed.app'));
    await tapKey(tester, 'nav-hosts');
    await tapKey(tester, 'nav-settings');
    expect(find.text('Zed'), findsOneWidget);
    await tapKey(tester, 'sftp-default-editor-reset');
    expect(settings.currentLocal.sftpDefaultEditor, isNull);
    expect(find.byKey(const ValueKey('sftp-default-editor-reset')), findsNothing);
    expect(settings.currentLocal.uiFontScale, 1.4);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
