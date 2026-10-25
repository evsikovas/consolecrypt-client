import 'dart:convert';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/vault/recovery_kit_print.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pdf/pdf.dart';

import '../helpers/test_app.dart';

/// Stands in for the system print dialog: builds the PDF once, like a
/// dialog would for the default paper.
class _FakePrinter implements RecoveryKitPrinter {
  _FakePrinter({this.available = true, this.error});

  final bool available;
  final Exception? error;
  final documents = <Uint8List>[];
  final headers = <String>[];
  final names = <String>[];

  @override
  Future<bool> canPrint() async => available;

  @override
  Future<bool> printDocument({
    required String name,
    required Future<Uint8List> Function(PdfPageFormat format) build,
  }) async {
    names.add(name);
    if (error != null) throw error!;
    final pdf = await build(PdfPageFormat.a4);
    headers.add(latin1.decode(pdf.sublist(0, 5)));
    documents.add(pdf);
    return true;
  }
}

void main() {
  final en = lookupAppLocalizations(const Locale('en'));

  setUpAll(() async {
    // Real asset I/O outside the per-widget fake-async zones.
    TestWidgetsFlutterBinding.ensureInitialized();
    await RecoveryKitFonts.load();
  });

  Future<MockBackend> openRecoveryKit(WidgetTester tester, _FakePrinter printer) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, overrides: [recoveryKitPrinterProvider.overrideWithValue(printer)]);
    await tester.tap(find.text(en.welcomeLocalAction));
    await settle(tester);
    await enterKey(tester, 'new-passphrase', 'violet-anchor-muffin-glacier-42');
    await enterKey(tester, 'confirm-passphrase', 'violet-anchor-muffin-glacier-42');
    await settle(tester);
    await tapKey(tester, 'create-vault');
    expect(find.byKey(const ValueKey('reveal-kit')), findsOneWidget);
    return backend;
  }

  // Leave the revealed kit while the app is alive (as onboarding does), so
  // the screen-capture guard is released before the tree is torn down.
  Future<void> continueToVerification(WidgetTester tester) async {
    await tapKey(tester, 'kit-saved');
    await tapKey(tester, 'kit-continue');
    expect(find.byKey(const ValueKey('kit-word-grid')), findsNothing);
  }

  testWidgets('print is offered only after the words are revealed and prints the kit from memory', (tester) async {
    final printer = _FakePrinter();
    await openRecoveryKit(tester, printer);
    expect(find.byKey(const ValueKey('kit-print')), findsNothing);
    expect(find.text(en.recoveryKitPrintHint), findsNothing);

    await tapKey(tester, 'reveal-kit');
    expect(find.byKey(const ValueKey('kit-print')), findsOneWidget);
    expect(find.text(en.recoveryKitPrint), findsOneWidget);
    expect(find.text(en.recoveryKitPrintHint), findsOneWidget);

    await tapKey(tester, 'kit-print');
    // Laying out the PDF is real work outside the fake clock.
    await tester.runAsync(() async {
      for (var i = 0; i < 200 && printer.documents.isEmpty; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 50));
      }
    });
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(printer.names, [en.recoveryKitSheetTitle]);
    expect(printer.headers, ['%PDF-']);
    // The PDF handed to the dialog is zeroed once the dialog closes.
    expect(printer.documents.single.every((byte) => byte == 0), isTrue);
    await continueToVerification(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('no print button where the system print dialog is unavailable', (tester) async {
    await openRecoveryKit(tester, _FakePrinter(available: false));
    await tapKey(tester, 'reveal-kit');
    expect(find.text(en.recoveryKitCopyWords), findsOneWidget);
    expect(find.byKey(const ValueKey('kit-print')), findsNothing);
    expect(find.text(en.recoveryKitPrintHint), findsNothing);
    await continueToVerification(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('a failed print dialog shows an error and keeps the kit on screen', (tester) async {
    final printer = _FakePrinter(error: PlatformException(code: 'print'));
    await openRecoveryKit(tester, printer);
    await tapKey(tester, 'reveal-kit');
    await tapKey(tester, 'kit-print');
    await settle(tester);
    expect(find.text(en.recoveryKitPrintFailed), findsOneWidget);
    expect(find.byKey(const ValueKey('kit-word-grid')), findsOneWidget);
    expect(printer.documents, isEmpty);
    await continueToVerification(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));
}
