import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/vault/recovery_kit_print.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart' show Locale;
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:pdf/pdf.dart';

/// Records what the system print dialog would have been given.
class _FakePrinter implements RecoveryKitPrinter {
  _FakePrinter({this.formats = const [PdfPageFormat.a4], this.error});

  final List<PdfPageFormat> formats;
  final Exception? error;
  final documents = <Uint8List>[];
  final firstBytes = <String>[];
  String? name;

  @override
  Future<bool> canPrint() async => true;

  @override
  Future<bool> printDocument({
    required String name,
    required Future<Uint8List> Function(PdfPageFormat format) build,
  }) async {
    this.name = name;
    // The dialog lays the document out again when the paper changes.
    for (final format in formats) {
      final pdf = await build(format);
      documents.add(pdf);
      firstBytes.add(latin1.decode(pdf.sublist(0, 5)));
    }
    if (error != null) throw error!;
    return true;
  }
}

RecoveryKit _kit({Uri? server}) => RecoveryKit(
  vaultId: const VaultId('print-fixture'),
  words: List.generate(RecoveryKit.wordCount, (index) => index.isEven ? 'abstract' : 'strategy'),
  qrPayload: 'print-fixture-not-a-recovery-key',
  createdAt: DateTime(2026, 10, 25, 22, 30),
  serverUrl: server,
);

/// The PDF's text with every compressed stream inflated (object streams
/// hold the font dictionaries).
String _inflated(Uint8List pdf) {
  final raw = latin1.decode(pdf);
  final text = StringBuffer(raw);
  for (final start in RegExp(r'(?<!end)stream\r?\n').allMatches(raw)) {
    final end = raw.indexOf('endstream', start.end);
    if (end < 0) continue;
    try {
      text.write(latin1.decode(zlib.decode(pdf.sublist(start.end, end))));
    } on FormatException {
      // Not a deflate stream.
    }
  }
  return text.toString();
}

bool _isPdf(Uint8List pdf) =>
    latin1.decode(pdf.sublist(0, 5)) == '%PDF-' && latin1.decode(pdf.sublist(pdf.length - 7)).contains('%%EOF');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final en = lookupAppLocalizations(const Locale('en'));
  final ru = lookupAppLocalizations(const Locale('ru'));
  setUpAll(() async {
    await initializeDateFormatting('en');
    await initializeDateFormatting('ru');
  });

  group('Recovery Kit PDF', () {
    for (final (name, l10n) in [('en', en), ('ru', ru)]) {
      for (final (paper, format) in [
        ('A4', PdfPageFormat.a4),
        ('Letter', PdfPageFormat.letter),
        ('A6', PdfPageFormat.a6),
        ('A4 landscape', PdfPageFormat.a4.landscape),
      ]) {
        test('$name on $paper is one valid page with embedded Roboto', () async {
          final kit = _kit(server: Uri.parse('https://sync.example.org'));
          addTearDown(kit.forget);
          final pdf = await buildRecoveryKitPdf(kit, l10n, format);
          expect(_isPdf(pdf), isTrue);
          final text = _inflated(pdf);
          expect(RegExp(r'/Count 1\b').hasMatch(text), isTrue, reason: 'one page');
          // The standard PDF fonts have no Cyrillic: both Roboto faces are embedded.
          expect(text, contains('/BaseFont/Roboto-Regular'));
          expect(text, contains('/BaseFont/Roboto-Bold'));
          expect(text, contains('/FontFile2'));
        });
      }
    }

    test('a local profile prints without a server', () async {
      final kit = _kit();
      addTearDown(kit.forget);
      expect(_isPdf(await buildRecoveryKitPdf(kit, ru, PdfPageFormat.a4)), isTrue);
    });
  });

  group('printRecoveryKit', () {
    test('names the job after the sheet and zeroes every PDF it built', () async {
      final kit = _kit();
      addTearDown(kit.forget);
      final printer = _FakePrinter(formats: [PdfPageFormat.a4, PdfPageFormat.letter]);
      expect(await printRecoveryKit(printer, kit, ru), isTrue);
      expect(printer.name, ru.recoveryKitSheetTitle);
      expect(printer.firstBytes, ['%PDF-', '%PDF-']);
      expect(printer.documents, hasLength(2));
      for (final pdf in printer.documents) {
        expect(pdf.every((byte) => byte == 0), isTrue);
      }
    });

    test('zeroes the PDF when the print dialog fails', () async {
      final kit = _kit();
      addTearDown(kit.forget);
      final printer = _FakePrinter(error: PlatformException(code: 'print'));
      await expectLater(printRecoveryKit(printer, kit, en), throwsA(isA<PlatformException>()));
      expect(printer.firstBytes, ['%PDF-']);
      expect(printer.documents.single.every((byte) => byte == 0), isTrue);
    });
  });

  group('no temporary files', () {
    test('only the print module uses `printing`, and never its file-based APIs', () {
      final users = Directory('lib')
          .listSync(recursive: true)
          .whereType<File>()
          .where((f) => f.path.endsWith('.dart') && f.readAsStringSync().contains('package:printing/'))
          .map((f) => f.path)
          .toList();
      expect(users, ['lib/vault/recovery_kit_print.dart']);

      final source = File('lib/vault/recovery_kit_print.dart').readAsStringSync();
      for (final forbidden in [
        "import 'dart:io",
        'path_provider',
        'Printing.sharePdf',
        'Printing.convertHtml',
        'Printing.raster',
        'PdfPreview',
        'writeAsBytes',
      ]) {
        expect(source.contains(forbidden), isFalse, reason: forbidden);
      }
    });

    test('printing stays pinned to the version whose native paths were checked', () {
      final pubspec = File('pubspec.yaml').readAsStringSync();
      expect(RegExp(r'^  printing: 5\.15\.1$', multiLine: true).hasMatch(pubspec), isTrue);
    });
  });
}
