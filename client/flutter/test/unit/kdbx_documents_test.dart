// Filesystem verification is deliberately asynchronous, outside widget frames.
// ignore_for_file: avoid_slow_async_io
import 'dart:io';

import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/keepass/kdbx_documents.dart';
import 'package:flutter_test/flutter_test.dart';

class _Files extends Fake implements FileDialogService {
  String? input;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async => input;
}

void main() {
  test('mobile import retains an independent encrypted copy after picker cleanup', () async {
    final root = await Directory.systemTemp.createTemp('cc-mobile-kdbx-');
    addTearDown(() => root.delete(recursive: true));
    final original = File('${root.path}/Picker.kdbx');
    await original.writeAsBytes([3, 217, 162, 154, 7]);
    final files = _Files()..input = original.path;
    final documents = KdbxDocuments(mobile: true, directory: () async => '${root.path}/private');
    final source = (await documents.choose(files))!;
    documents.keep(source);
    await original.delete();
    expect(await File(source.path).readAsBytes(), [3, 217, 162, 154, 7]);
    await documents.discard(source); // Already committed: never remove it.
    expect(await File(source.path).exists(), isTrue);
  });

  test('mobile cancelled unlock removes only its own staged copy, not the picker file', () async {
    final root = await Directory.systemTemp.createTemp('cc-mobile-kdbx-');
    addTearDown(() => root.delete(recursive: true));
    final original = File('${root.path}/Picker.kdbx');
    await original.writeAsBytes([1, 2, 3]);
    final files = _Files()..input = original.path;
    final documents = KdbxDocuments(mobile: true, directory: () async => '${root.path}/private');
    final source = (await documents.choose(files))!;
    await documents.discard(source);
    expect(await File(source.path).exists(), isFalse);
    expect(await original.readAsBytes(), [1, 2, 3]);
    final created = (await documents.choose(files, create: true))!;
    expect(await File(created.path).parent.exists(), isTrue);
    expect(await File(created.path).exists(), isFalse);
    await documents.discard(created);
    files.input = null;
    expect(await documents.choose(files), isNull);
  });
}
