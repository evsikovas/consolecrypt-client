// Public guide illustration. Only synthetic, unusable recovery data is rendered.
// flutter test --dart-define=CC_GUIDE_OUTPUT=/absolute/output tool/recovery_kit_preview.dart
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/vault/recovery_kit_screen.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const _output = String.fromEnvironment('CC_GUIDE_OUTPUT');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    if (_output.isEmpty) throw StateError('Set CC_GUIDE_OUTPUT to a private capture directory.');
    final flutterRoot = Platform.environment['FLUTTER_ROOT'] ?? '/opt/homebrew/share/flutter';
    for (final font in [
      ('Roboto', '$flutterRoot/bin/cache/artifacts/material_fonts/Roboto-Regular.ttf'),
      ('monospace', '/System/Library/Fonts/SFNSMono.ttf'),
    ]) {
      await (FontLoader(font.$1)..addFont(File(font.$2).readAsBytes().then(ByteData.sublistView))).load();
    }
    await Directory(_output).create(recursive: true);
  });
  for (final language in ['ru', 'en']) {
    testWidgets('mobile Recovery Kit $language', (tester) async {
      tester.view.physicalSize = const Size(412, 915);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final kit = RecoveryKit(
        vaultId: const VaultId('public-layout-example-not-a-vault'),
        words: List.generate(24, (index) => index.isEven ? 'abstract' : 'strategy'),
        qrPayload: 'ConsoleCrypt layout example. This is not a recovery key.',
        createdAt: DateTime.utc(2026, 10, 20),
      );
      addTearDown(kit.forget);
      final boundaryKey = GlobalKey();
      final l10n = lookupAppLocalizations(Locale(language));
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundaryKey,
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: AppTheme.build(Brightness.light, platform: TargetPlatform.android),
            builder: (context, child) => GlassScope(
              data: GlassScopeData(appearance: GlassAppearance.fallback, budget: GlassBackdropBudget()),
              child: child!,
            ),
            home: Scaffold(
              backgroundColor: const Color(0xFFF8FAFC),
              body: Padding(
                padding: const EdgeInsets.fromLTRB(40, 36, 40, 24),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(l10n.recoveryKitTitle, style: const TextStyle(fontSize: 25, fontWeight: FontWeight.w600)),
                    const SizedBox(height: 12),
                    Text(
                      language == 'ru'
                          ? 'Демонстрационный пример. Эти слова не открывают хранилище.'
                          : 'Demonstration only. These words cannot unlock a vault.',
                    ),
                    const SizedBox(height: 24),
                    RecoveryKitPaper(kit: kit),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(
        tester.getRect(find.byKey(const ValueKey('kit-qr'))).top,
        greaterThan(tester.getRect(find.byKey(const ValueKey('kit-word-23'))).bottom),
      );
      await tester.runAsync(() async {
        final boundary = boundaryKey.currentContext!.findRenderObject()! as RenderRepaintBoundary;
        final image = await boundary.toImage();
        final bytes = (await image.toByteData(format: ui.ImageByteFormat.png))!.buffer.asUint8List();
        await File('$_output/recovery-kit-mobile-$language.png').writeAsBytes(bytes);
        image.dispose();
      });
    });
  }
}
