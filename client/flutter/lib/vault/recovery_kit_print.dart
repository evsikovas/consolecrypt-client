import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pdf/pdf.dart';
import 'package:pdf/widgets.dart' as pw;
import 'package:printing/printing.dart';

/// Print / "Save as PDF" of the Recovery Kit (ADR-0002: vault ID, words, QR,
/// server, date).
///
/// The PDF is built in memory and handed straight to the system print dialog;
/// the app never writes it to a file. `printing` is pinned to the version
/// whose native print paths were checked for that on every platform (see
/// pubspec.yaml). "Save as PDF" is the system dialog's own choice, written
/// where the user picks.
abstract interface class RecoveryKitPrinter {
  /// Whether the system print dialog is available on this device.
  Future<bool> canPrint();

  /// Opens the system print dialog. [build] lays the kit out for the paper
  /// the user picks and may run more than once. Returns false when the user
  /// cancels.
  Future<bool> printDocument({required String name, required Future<Uint8List> Function(PdfPageFormat format) build});
}

final recoveryKitPrinterProvider = Provider<RecoveryKitPrinter>((ref) => const SystemRecoveryKitPrinter());

/// Whether to offer printing: false while the answer is unknown or the
/// dialog is unavailable.
final recoveryKitCanPrintProvider = FutureProvider.autoDispose<bool>(
  (ref) => ref.watch(recoveryKitPrinterProvider).canPrint(),
);

/// The `printing` plugin's [Printing.layoutPdf] only — never `sharePdf`,
/// `convertHtml` or the raster preview, which go through temporary files.
class SystemRecoveryKitPrinter implements RecoveryKitPrinter {
  const SystemRecoveryKitPrinter();

  @override
  Future<bool> canPrint() async {
    // The web implementation was not checked; desktop and mobile were.
    if (kIsWeb) return false;
    try {
      return (await Printing.info()).canPrint;
    } on PlatformException {
      return false;
    } on MissingPluginException {
      return false;
    }
  }

  @override
  Future<bool> printDocument({required String name, required Future<Uint8List> Function(PdfPageFormat format) build}) =>
      Printing.layoutPdf(onLayout: build, name: name, usePrinterSettings: true);
}

/// Prints [kit] through [printer]. Every PDF built for the dialog is zeroed
/// once the dialog closes (best effort: the platform keeps its own copy
/// until the job is spooled).
Future<bool> printRecoveryKit(RecoveryKitPrinter printer, RecoveryKit kit, AppLocalizations l10n) async {
  final built = <Uint8List>[];
  try {
    return await printer.printDocument(
      name: l10n.recoveryKitSheetTitle,
      build: (format) async {
        final pdf = await buildRecoveryKitPdf(kit, l10n, format);
        built.add(pdf);
        return pdf;
      },
    );
  } finally {
    for (final pdf in built) {
      pdf.fillRange(0, pdf.length, 0);
    }
  }
}

/// Roboto (Apache-2.0, assets/fonts): the PDF standard fonts have no
/// Cyrillic. Loaded once per process.
abstract final class RecoveryKitFonts {
  static Future<({pw.Font regular, pw.Font bold})>? _fonts;

  static Future<({pw.Font regular, pw.Font bold})> load() => _fonts ??= () async {
    final regular = await rootBundle.load('assets/fonts/Roboto-Regular.ttf');
    final bold = await rootBundle.load('assets/fonts/Roboto-Bold.ttf');
    return (regular: pw.Font.ttf(regular), bold: pw.Font.ttf(bold));
  }();
}

/// One page: title, warning, details, the 24 numbered words and the QR code.
/// The words keep the on-screen reading order (row by row).
Future<Uint8List> buildRecoveryKitPdf(RecoveryKit kit, AppLocalizations l10n, PdfPageFormat format) async {
  final fonts = await RecoveryKitFonts.load();
  final document = pw.Document(
    title: l10n.recoveryKitSheetTitle,
    creator: 'ConsoleCrypt',
    theme: pw.ThemeData.withFont(base: fonts.regular, bold: fonts.bold),
  );
  // Printers report their unprintable edges; keep at least 1.5 cm anyway.
  const minMargin = 1.5 * PdfPageFormat.cm;
  final page = format.copyWith(
    marginLeft: math.max(format.marginLeft, minMargin),
    marginTop: math.max(format.marginTop, minMargin),
    marginRight: math.max(format.marginRight, minMargin),
    marginBottom: math.max(format.marginBottom, minMargin),
  );
  const ink = PdfColors.black;
  const muted = PdfColors.grey700;
  const hairline = PdfColors.grey400;
  final words = kit.exposeWords();

  pw.Widget detail(String label, String value) => pw.Padding(
    padding: const pw.EdgeInsets.symmetric(vertical: 3),
    child: pw.Row(
      crossAxisAlignment: pw.CrossAxisAlignment.start,
      children: [
        pw.SizedBox(
          width: 110,
          child: pw.Text(label, style: const pw.TextStyle(fontSize: 10, color: muted)),
        ),
        pw.Expanded(
          child: pw.Text(value, style: const pw.TextStyle(fontSize: 10, color: ink)),
        ),
      ],
    ),
  );

  pw.Widget wordGrid(double width) {
    const gap = 12.0;
    final columns = ((width + gap) / (96 + gap)).floor().clamp(1, 4);
    final rows = (words.length / columns).ceil();
    return pw.Column(
      children: [
        for (var row = 0; row < rows; row++)
          pw.Padding(
            padding: const pw.EdgeInsets.symmetric(vertical: 4),
            child: pw.Row(
              children: [
                for (var column = 0; column < columns; column++) ...[
                  if (column > 0) pw.SizedBox(width: gap),
                  pw.Expanded(
                    child: row * columns + column < words.length
                        ? pw.Row(
                            children: [
                              pw.SizedBox(
                                width: 18,
                                child: pw.Text(
                                  '${row * columns + column + 1}',
                                  textAlign: pw.TextAlign.right,
                                  style: const pw.TextStyle(fontSize: 9, color: muted),
                                ),
                              ),
                              pw.SizedBox(width: 6),
                              pw.Text(
                                words[row * columns + column],
                                style: const pw.TextStyle(fontSize: 13, fontWeight: pw.FontWeight.bold, color: ink),
                              ),
                            ],
                          )
                        : pw.SizedBox(),
                  ),
                ],
              ],
            ),
          ),
      ],
    );
  }

  const qrSize = 132.0;
  final qr = pw.BarcodeWidget(
    barcode: pw.Barcode.qrCode(),
    data: kit.exposeQrPayload(),
    width: qrSize,
    height: qrSize,
    drawText: false,
    // Quiet zone: keeps the frame line out of the scanner's way.
    padding: const pw.EdgeInsets.all(8),
  );

  // Small paper (A6, photo sizes) gets the same layout scaled down: the kit
  // always stays on one page.
  final layoutWidth = math.max(page.availableWidth, 400.0);
  document.addPage(
    pw.Page(
      pageFormat: page,
      build: (context) => pw.FittedBox(
        fit: pw.BoxFit.scaleDown,
        alignment: pw.Alignment.topLeft,
        child: pw.SizedBox(
          width: layoutWidth,
          child: pw.Column(
            mainAxisSize: pw.MainAxisSize.min,
            crossAxisAlignment: pw.CrossAxisAlignment.start,
            children: [
              pw.Text(
                l10n.recoveryKitSheetTitle,
                style: const pw.TextStyle(fontSize: 20, fontWeight: pw.FontWeight.bold, color: ink),
              ),
              pw.SizedBox(height: 8),
              pw.Text(l10n.recoveryKitSheetWarning, style: const pw.TextStyle(fontSize: 11, color: ink)),
              pw.SizedBox(height: 16),
              pw.Container(
                padding: const pw.EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                decoration: pw.BoxDecoration(
                  border: pw.Border.all(color: hairline, width: 0.75),
                  borderRadius: pw.BorderRadius.circular(6),
                ),
                child: pw.Column(
                  children: [
                    detail(l10n.recoveryKitVaultId, kit.vaultId.value),
                    detail(l10n.recoveryKitServer, kit.serverUrl?.toString() ?? l10n.recoveryKitNoServerCopy),
                    detail(l10n.recoveryKitCreated, formatDateTime(l10n, kit.createdAt)),
                  ],
                ),
              ),
              pw.SizedBox(height: 16),
              pw.Container(
                padding: const pw.EdgeInsets.all(14),
                decoration: pw.BoxDecoration(
                  border: pw.Border.all(color: hairline, width: 0.75),
                  borderRadius: pw.BorderRadius.circular(6),
                ),
                child: pw.LayoutBuilder(
                  builder: (context, constraints) {
                    final width = constraints!.maxWidth;
                    // The QR sits beside the words when four columns still fit.
                    if (width >= 4 * 96 + 3 * 12 + 16 + qrSize) {
                      return pw.Row(
                        crossAxisAlignment: pw.CrossAxisAlignment.start,
                        children: [
                          pw.Expanded(child: wordGrid(width - qrSize - 16)),
                          pw.SizedBox(width: 16),
                          qr,
                        ],
                      );
                    }
                    return pw.Column(children: [wordGrid(width), pw.SizedBox(height: 14), qr]);
                  },
                ),
              ),
              pw.SizedBox(height: 16),
              pw.Text(l10n.recoveryKitKeyNotStored, style: const pw.TextStyle(fontSize: 9, color: muted)),
            ],
          ),
        ),
      ),
    ),
  );
  return document.save();
}
