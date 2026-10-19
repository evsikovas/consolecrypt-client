import 'dart:io';

/// Resolved before the Rust core or the local model manager can create files.
/// Null roots keep the existing platform defaults outside macOS.
final class AppStoragePaths {
  const AppStoragePaths({this.coreRoot, this.localAiRoot});

  final Directory? coreRoot;
  final Directory? localAiRoot;

  /// Explicit application roots are also used by native integration tests.
  /// Keep their model storage isolated from the user's real home directory.
  factory AppStoragePaths.explicit(String coreRoot) =>
      AppStoragePaths(coreRoot: Directory(coreRoot), localAiRoot: Directory('$coreRoot/local-ai'));

  /// Updates from the sandboxed macOS app reuse its populated container in
  /// place. No vault/model copying, renaming, merging, or settings rewrite is
  /// necessary. Core and AI can independently use their old or new location.
  static Future<AppStoragePaths> resolve({bool? isMacOS, Map<String, String>? environment}) async {
    if (!(isMacOS ?? Platform.isMacOS)) return const AppStoragePaths();
    final env = environment ?? Platform.environment;
    var home = env['HOME'];
    if (home == null || home.isEmpty || !home.startsWith('/')) {
      throw const AppStorageException(AppStorageFailure.homeUnavailable);
    }
    while (home!.endsWith('/') && home.length > 1) {
      home = home.substring(0, home.length - 1);
    }
    const containerSuffix = '/Library/Containers/io.consolecrypt.consolecrypt/Data';
    // Also handle a launcher carrying the previous sandbox HOME. Never nest a
    // second Library/Containers below the former container's Data directory.
    if (home.endsWith(containerSuffix)) {
      home = home.substring(0, home.length - containerSuffix.length);
    }
    if (home.isEmpty) throw const AppStorageException(AppStorageFailure.homeUnavailable);
    final legacyHome = '$home$containerSuffix';
    final explicitCore = env['CONSOLECRYPT_DATA_DIR'];
    final core = explicitCore != null && explicitCore.trim().isNotEmpty
        ? Directory(explicitCore).absolute
        : await _select(
            Directory(
              '$home/Library/Application Support/io.consolecrypt.ConsoleCrypt', // l10n-ignore: platform filesystem path
            ),
            Directory(
              '$legacyHome/Library/Application Support/io.consolecrypt.ConsoleCrypt', // l10n-ignore: platform filesystem path
            ),
            AppStorageFailure.coreConflict,
          );
    final ai = await _select(
      Directory('$home/Library/Application Support/ConsoleCrypt/local-ai'), // l10n-ignore: platform filesystem path
      Directory(
        '$legacyHome/Library/Application Support/ConsoleCrypt/local-ai', // l10n-ignore: platform filesystem path
      ),
      AppStorageFailure.localAiConflict,
    );
    // The bridge intentionally defaults missing UI state. A denied existing
    // file must not be mistaken for missing state and reset the local-AI
    // preference (including an explicitly stored false).
    await _checkReadableFile(File('${core.path}/ui-state.json'));
    await _checkReadableFile(File('${core.path}/profiles.json'));
    return AppStoragePaths(coreRoot: core, localAiRoot: ai);
  }

  static Future<Directory> _select(Directory current, Directory legacy, AppStorageFailure conflict) async {
    final currentHasData = await _hasData(current);
    final legacyHasData = await _hasData(legacy);
    if (currentHasData && legacyHasData) {
      // An intentional filesystem alias is one root, not two divergent copies.
      try {
        if (await current.resolveSymbolicLinks() == await legacy.resolveSymbolicLinks()) return current;
      } on FileSystemException {
        throw const AppStorageException(AppStorageFailure.unavailable);
      }
      throw AppStorageException(conflict);
    }
    return legacyHasData ? legacy : current;
  }

  static Future<bool> _hasData(Directory root) async {
    try {
      // exists()/stat() collapse some access errors to "not found". Listing
      // raises EACCES/EPERM even if an ancestor is a protected macOS container.
      return !(await root.list(followLinks: false).isEmpty);
    } on FileSystemException catch (error) {
      if (error.osError?.errorCode == 2) return false; // ENOENT only.
      throw const AppStorageException(AppStorageFailure.unavailable);
    }
  }

  static Future<void> _checkReadableFile(File file) async {
    try {
      final handle = await file.open();
      await handle.close();
    } on FileSystemException catch (error) {
      if (error.osError?.errorCode != 2) {
        throw const AppStorageException(AppStorageFailure.unavailable);
      }
    }
  }
}

enum AppStorageFailure { homeUnavailable, unavailable, coreConflict, localAiConflict }

/// Safe startup diagnostics contain no usernames, vault contents, or raw OS
/// error payloads. The startup screen is displayed before locale state opens.
final class AppStorageException implements Exception {
  const AppStorageException(this.failure);
  final AppStorageFailure failure;

  String get message => switch (failure) {
    AppStorageFailure.homeUnavailable =>
      'The macOS home directory could not be determined.', // l10n-ignore: shown before localization loads
    AppStorageFailure.unavailable => 'Existing ConsoleCrypt storage cannot be read. Check filesystem permissions and macOS access to the previous app container, then retry. No new profile was created.', // l10n-ignore: shown before localization loads
    AppStorageFailure.coreConflict => 'Both the previous app container and the standard Application Support location contain ConsoleCrypt profiles or settings. Resolve these duplicate storage locations before starting; no data was merged or moved.', // l10n-ignore: shown before localization loads
    AppStorageFailure.localAiConflict => 'Both the previous app container and the standard Application Support location contain local AI data. Resolve these duplicate storage locations before starting; no models were merged or moved.', // l10n-ignore: shown before localization loads
  };

  @override
  String toString() => 'AppStorageException(${failure.name})';
}
