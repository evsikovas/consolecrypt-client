import 'dart:io';
import 'dart:typed_data';

import 'package:file_selector/file_selector.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Destination selection and mobile completion stay with FileDialogService.
/// This small injectable sink receives only already-issued Native JSON bytes.
abstract interface class InventoryExportFileWriter {
  Future<void> write(String path, Uint8List bytes);
  Future<void> discardMobileStaging(String path);
}

final inventoryExportFileWriterProvider = Provider<InventoryExportFileWriter>((_) => const _FileWriter());

final class _FileWriter implements InventoryExportFileWriter {
  const _FileWriter();
  @override
  Future<void> write(String path, Uint8List bytes) =>
      XFile.fromData(bytes, mimeType: 'application/json', name: 'inventory.consolecrypt.json').saveTo(path);
  @override
  Future<void> discardMobileStaging(String path) async {
    try {
      await File(path).delete();
    } on FileSystemException {
      /* Only private file-selector staging. */
    }
  }
}
