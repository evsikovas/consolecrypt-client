import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/vault/recovery_kit_screen.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  for (final width in [320.0, 412.0, 844.0, 1280.0]) {
    for (final scale in [1.0, 1.5, 2.0]) {
      for (final brightness in Brightness.values) {
        testWidgets('recovery words remain whole at $width / $scale / ${brightness.name}', (tester) async {
          tester.view.physicalSize = Size(width, 900);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.reset);
          final kit = RecoveryKit(
            vaultId: const VaultId('layout-fixture'),
            words: List.generate(24, (index) => index.isEven ? 'abstract' : 'strategy'),
            qrPayload: 'layout-fixture-not-a-recovery-key',
            createdAt: DateTime(2026),
          );
          addTearDown(kit.forget);
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.build(brightness, platform: TargetPlatform.android),
              builder: (context, child) => MediaQuery(
                data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
                child: GlassScope(
                  data: GlassScopeData(appearance: GlassAppearance.fallback, budget: GlassBackdropBudget()),
                  child: child!,
                ),
              ),
              home: Scaffold(
                body: SingleChildScrollView(
                  padding: const EdgeInsets.all(16),
                  child: RecoveryKitPaper(kit: kit),
                ),
              ),
            ),
          );
          await tester.pump();
          expect(tester.takeException(), isNull);
          final qr = tester.getRect(find.byKey(const ValueKey('kit-qr')));
          final rectangles = <Rect>[];
          for (var index = 0; index < RecoveryKit.wordCount; index++) {
            final finder = find.byKey(ValueKey('kit-word-$index'));
            expect(finder, findsOneWidget);
            final paragraph = tester.renderObject<RenderParagraph>(finder);
            // An overflow-free Row alone is insufficient: its text can still
            // wrap into individual letters or be clipped in a tiny Expanded.
            expect(paragraph.size.width + 0.01, greaterThanOrEqualTo(paragraph.getMaxIntrinsicWidth(double.infinity)));
            expect(paragraph.didExceedMaxLines, isFalse);
            final rect = tester.getRect(finder);
            expect(rect.overlaps(qr), isFalse);
            expect(rectangles.any(rect.overlaps), isFalse);
            rectangles.add(rect);
          }
          if (width <= 412) {
            expect(qr.top, greaterThan(rectangles.last.bottom));
          }
        });
      }
    }
  }
}
