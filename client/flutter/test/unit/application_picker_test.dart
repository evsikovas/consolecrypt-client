import 'package:consolecrypt/core/bridge/rust_backup.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:file_selector_platform_interface/file_selector_platform_interface.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

class _Applications extends FileSelectorPlatform {
  XFile? selection;
  PlatformException? error;
  List<XTypeGroup>? groups;
  String? directory;
  String? confirmLabel;

  @override
  Future<XFile?> openFile({
    List<XTypeGroup>? acceptedTypeGroups,
    String? initialDirectory,
    String? confirmButtonText,
  }) async {
    groups = acceptedTypeGroups;
    directory = initialDirectory;
    confirmLabel = confirmButtonText;
    if (error != null) throw error!;
    return selection;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late _Applications picker;
  setUp(() {
    final previous = FileSelectorPlatform.instance;
    picker = _Applications();
    FileSelectorPlatform.instance = picker;
    addTearDown(() => FileSelectorPlatform.instance = previous);
  });
  const service = NativeFileDialogService();
  test('native panel filters app bundles, starts in Applications and returns paths unchanged', () async {
    for (final path in [
      '/Applications/Visual Studio Code.app',
      '/Applications/Zed.app',
      '/Users/demo/Мои программы/Editor.app',
    ]) {
      picker.selection = XFile(path);
      expect(await service.chooseApplication(label: 'Программы', confirmButtonText: 'Выбрать программу'), path);
      expect(picker.directory, '/Applications');
      expect(picker.confirmLabel, 'Выбрать программу');
      expect(picker.groups!.single.uniformTypeIdentifiers, ['com.apple.application-bundle']);
    }
    picker.selection = null;
    expect(await service.chooseApplication(label: 'Applications', confirmButtonText: 'Choose'), isNull);
  });
  test('picker and launch failures show a specific localized error with the diagnostic', () async {
    picker.error = PlatformException(code: 'panel_failed', message: 'Unable to show panel');
    final ru = lookupAppLocalizations(const Locale('ru'));
    try {
      await service.chooseApplication(label: 'Applications', confirmButtonText: 'Choose');
      fail('expected a picker error');
    } on AppException catch (e) {
      expect(e.reason, AppErrorReason.applicationPickerFailed);
      expect(errorMessage(ru, e), contains('Не удалось открыть выбор программы'));
      expect(errorMessage(ru, e), contains('Unable to show panel'));
    }
    const error = AppException(
      AppErrorCode.internal,
      'diagnostic',
      reason: AppErrorReason.editorLaunchFailed,
      args: {'detail': 'Application not found'},
    );
    expect(errorMessage(ru, error), contains('Не удалось открыть редактор'));
    expect(errorMessage(ru, error), contains('Application not found'));
    expect(errorMessage(ru, error), isNot(ru.errorInternal));
  });
}
