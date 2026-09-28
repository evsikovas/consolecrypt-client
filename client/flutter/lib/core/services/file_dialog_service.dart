/// Native open/save/folder dialogs. Returns `null` when the user cancels.
///
/// The desktop implementation will use the flutter.dev `file_selector`
/// plugin (BSD-3-Clause) at M4; the mock returns demo paths (ADR-0101).
abstract interface class FileDialogService {
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions});

  /// Finish a save: Android exports the staged file via its document picker.
  /// Returns false when the user cancels; desktop files are already in place.
  Future<bool> finishSaveFile(String path);

  Future<String?> chooseOpenFile({List<String> extensions});

  Future<String?> chooseDirectory();

  /// macOS: select an application bundle with the native file panel. This
  /// includes non-scriptable editors and apps outside /Applications.
  Future<String?> chooseApplication({required String label, required String confirmButtonText});
}
