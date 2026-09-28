import 'package:consolecrypt/core/models/sftp_browser.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:material_ui/material_ui.dart';

/// Broad kind of a file, derived from its name / mode (Kind column, icon,
/// Quick Look strategy).
enum FileKindGroup {
  folder,
  code,
  script,
  text,
  config,
  log,
  image,
  pdf,
  document,
  archive,
  audio,
  video,
  font,
  key,
  database,
  executable,
  symlink,
  special,
  generic,
}

final class FileKind {
  const FileKind(this.group, [this.format]);

  final FileKindGroup group;

  /// Technical token shown as is (`PHP`, `PNG`, `TAR.GZ`); never translated.
  final String? format;

  /// Locale-independent sort key (Kind column).
  String get sortKey => '${group.index.toString().padLeft(2, '0')}:${format ?? ''}';

  /// Text / code worth showing in Quick Look without sniffing.
  bool get isTextual => switch (group) {
    FileKindGroup.code ||
    FileKindGroup.script ||
    FileKindGroup.text ||
    FileKindGroup.config ||
    FileKindGroup.log ||
    FileKindGroup.key => true,
    _ => format == 'SVG',
  };

  /// Raster images Quick Look decodes.
  bool get isImage => group == FileKindGroup.image && format != 'SVG';

  /// Binary formats Quick Look does not try to read.
  bool get isKnownBinary => switch (group) {
    FileKindGroup.pdf ||
    FileKindGroup.archive ||
    FileKindGroup.audio ||
    FileKindGroup.video ||
    FileKindGroup.font ||
    FileKindGroup.database => true,
    _ => false,
  };

  String label(AppLocalizations l) => switch (group) {
    FileKindGroup.folder => l.sftpKindFolder,
    FileKindGroup.code => format!,
    FileKindGroup.script => l.sftpKindShellScript,
    FileKindGroup.text => l.sftpKindText,
    FileKindGroup.config => l.sftpKindConfig,
    FileKindGroup.log => l.sftpKindLog,
    FileKindGroup.image => l.sftpKindImage(format!),
    FileKindGroup.pdf => l.sftpKindPdf,
    FileKindGroup.document => format == null ? l.sftpKindDocument : l.sftpKindDocumentFormat(format!),
    FileKindGroup.archive => l.sftpKindArchive(format!),
    FileKindGroup.audio => l.sftpKindAudio,
    FileKindGroup.video => l.sftpKindVideo,
    FileKindGroup.font => l.sftpKindFont,
    FileKindGroup.key => l.sftpKindKey,
    FileKindGroup.database => l.sftpKindDatabase,
    FileKindGroup.executable => l.sftpKindExecutable,
    FileKindGroup.symlink => l.sftpKindSymlink,
    FileKindGroup.special => l.sftpKindSpecial,
    FileKindGroup.generic => l.sftpKindDocument,
  };

  IconData get icon => switch (group) {
    FileKindGroup.folder => Icons.folder,
    FileKindGroup.code => Icons.code,
    FileKindGroup.script => Icons.terminal,
    FileKindGroup.text => Icons.description_outlined,
    FileKindGroup.config => Icons.tune,
    FileKindGroup.log => Icons.receipt_long_outlined,
    FileKindGroup.image => Icons.image_outlined,
    FileKindGroup.pdf => Icons.picture_as_pdf_outlined,
    FileKindGroup.document => Icons.article_outlined,
    FileKindGroup.archive => Icons.folder_zip_outlined,
    FileKindGroup.audio => Icons.audio_file_outlined,
    FileKindGroup.video => Icons.video_file_outlined,
    FileKindGroup.font => Icons.font_download_outlined,
    FileKindGroup.key => Icons.key_outlined,
    FileKindGroup.database => Icons.storage_outlined,
    FileKindGroup.executable => Icons.settings_applications_outlined,
    FileKindGroup.symlink => Icons.link,
    FileKindGroup.special => Icons.memory,
    FileKindGroup.generic => Icons.insert_drive_file_outlined,
  };

  /// Icon tint (folders blue like Finder; a few groups get a hue for
  /// scanning, the rest follow the text colour).
  Color color(ColorScheme scheme) => switch (group) {
    FileKindGroup.folder => const Color(0xFF3D8FE0),
    FileKindGroup.image => const Color(0xFF9C5FD1),
    FileKindGroup.archive => const Color(0xFFB7792B),
    FileKindGroup.code || FileKindGroup.script => scheme.primary,
    FileKindGroup.pdf => const Color(0xFFD0443A),
    FileKindGroup.executable => const Color(0xFF3A9D5D),
    _ => scheme.onSurfaceVariant,
  };
}

const _code = {
  'php': 'PHP',
  'html': 'HTML',
  'htm': 'HTML',
  'css': 'CSS',
  'scss': 'SCSS',
  'less': 'LESS',
  'js': 'JavaScript',
  'mjs': 'JavaScript',
  'cjs': 'JavaScript',
  'jsx': 'JSX',
  'ts': 'TypeScript',
  'tsx': 'TSX',
  'vue': 'Vue',
  'json': 'JSON',
  'yml': 'YAML',
  'yaml': 'YAML',
  'toml': 'TOML',
  'xml': 'XML',
  'md': 'Markdown',
  'markdown': 'Markdown',
  'py': 'Python',
  'rb': 'Ruby',
  'go': 'Go',
  'rs': 'Rust',
  'java': 'Java',
  'kt': 'Kotlin',
  'swift': 'Swift',
  'c': 'C',
  'h': 'C',
  'cc': 'C++',
  'cpp': 'C++',
  'hpp': 'C++',
  'cs': 'C#',
  'sql': 'SQL',
  'lua': 'Lua',
  'pl': 'Perl',
  'dart': 'Dart',
  'tf': 'Terraform',
  'twig': 'Twig',
  'blade': 'Blade',
};

const _scripts = {'sh', 'bash', 'zsh', 'fish', 'ksh', 'ps1', 'bat', 'cmd'};
const _text = {'txt', 'text', 'csv', 'tsv', 'rst', 'nfo', 'diff', 'patch'};
const _config = {'conf', 'cfg', 'ini', 'env', 'properties', 'service', 'socket', 'timer', 'plist', 'cnf', 'rules'};
const _images = {
  'png': 'PNG',
  'jpg': 'JPEG',
  'jpeg': 'JPEG',
  'gif': 'GIF',
  'webp': 'WebP',
  'bmp': 'BMP',
  'ico': 'ICO',
  'svg': 'SVG',
  'tif': 'TIFF',
  'tiff': 'TIFF',
  'heic': 'HEIC',
  'avif': 'AVIF',
};
const _archives = {
  'zip',
  'tar',
  'gz',
  'tgz',
  'bz2',
  'xz',
  'zst',
  '7z',
  'rar',
  'deb',
  'rpm',
  'jar',
  'war',
  'apk',
  'dmg',
};
const _audio = {'mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac', 'opus'};
const _video = {'mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'};
const _fonts = {'ttf', 'otf', 'woff', 'woff2'};
const _keys = {'pem', 'crt', 'cer', 'key', 'pub', 'p12', 'pfx', 'csr', 'gpg', 'asc'};
const _keyNames = {'authorized_keys', 'known_hosts', 'id_rsa', 'id_ed25519', 'id_ecdsa'};
const _databases = {'sqlite', 'sqlite3', 'db', 'mdb', 'dump', 'rdb'};
const _documents = {
  'doc': 'Word',
  'docx': 'Word',
  'xls': 'Excel',
  'xlsx': 'Excel',
  'ppt': 'PowerPoint',
  'pptx': 'PowerPoint',
  'odt': 'ODT',
  'ods': 'ODS',
  'rtf': 'RTF',
  'epub': 'EPUB',
};
const _shellDotfiles = {'.bashrc', '.bash_profile', '.bash_logout', '.profile', '.zshrc', '.zprofile', '.bash_aliases'};
const _configNames = {
  '.htaccess',
  '.gitignore',
  '.gitattributes',
  '.editorconfig',
  '.env',
  '.npmrc',
  'config',
  'hosts',
  'fstab',
  'crontab',
  'os-release',
  'sshd_config',
  'ssh_config',
  'nginx.conf',
  'default',
};
const _textNames = {'readme', 'license', 'licence', 'changelog', 'authors', 'notice', 'todo', 'copying'};
const _codeNames = {'dockerfile': 'Dockerfile', 'makefile': 'Makefile', 'vagrantfile': 'Ruby', 'gemfile': 'Ruby'};

/// Kind of a file named [name] (lower-cased extension logic, a few
/// well-known file names).
FileKind fileKindForName(String name, {bool executable = false}) {
  final lower = name.toLowerCase();
  final dot = lower.lastIndexOf('.');
  final ext = dot <= 0 ? '' : lower.substring(dot + 1);
  if (_keyNames.contains(lower)) return const FileKind(FileKindGroup.key);
  if (_shellDotfiles.contains(lower)) return const FileKind(FileKindGroup.script);
  if (_configNames.contains(lower)) return const FileKind(FileKindGroup.config);
  final codeName = _codeNames[lower];
  if (codeName != null) return FileKind(FileKindGroup.code, codeName);
  final tar = lower.lastIndexOf('.tar.');
  if (tar > 0 && RegExp(r'\.tar\.(gz|bz2|xz|zst)$').hasMatch(lower)) {
    return FileKind(FileKindGroup.archive, lower.substring(tar + 1).toUpperCase());
  }
  if (ext.isEmpty) {
    if (_textNames.contains(lower)) return const FileKind(FileKindGroup.text);
    if (lower.endsWith('log') || lower == 'messages') return const FileKind(FileKindGroup.log);
    return executable ? const FileKind(FileKindGroup.executable) : const FileKind(FileKindGroup.generic);
  }
  final code = _code[ext];
  if (code != null) return FileKind(FileKindGroup.code, code);
  final image = _images[ext];
  if (image != null) return FileKind(FileKindGroup.image, image);
  final document = _documents[ext];
  if (document != null) return FileKind(FileKindGroup.document, document);
  if (_scripts.contains(ext)) return const FileKind(FileKindGroup.script);
  if (_text.contains(ext)) return const FileKind(FileKindGroup.text);
  if (_config.contains(ext)) return const FileKind(FileKindGroup.config);
  if (ext == 'log' || RegExp(r'\.log\.\d+$').hasMatch(lower)) return const FileKind(FileKindGroup.log);
  if (ext == 'pdf') return const FileKind(FileKindGroup.pdf);
  if (_archives.contains(ext)) return FileKind(FileKindGroup.archive, ext.toUpperCase());
  if (_audio.contains(ext)) return const FileKind(FileKindGroup.audio);
  if (_video.contains(ext)) return const FileKind(FileKindGroup.video);
  if (_fonts.contains(ext)) return const FileKind(FileKindGroup.font);
  if (_keys.contains(ext)) return const FileKind(FileKindGroup.key);
  if (_databases.contains(ext)) return const FileKind(FileKindGroup.database);
  if (ext == 'exe' || ext == 'bin' || ext == 'appimage') return const FileKind(FileKindGroup.executable);
  return executable ? const FileKind(FileKindGroup.executable) : const FileKind(FileKindGroup.generic);
}

/// Kind of a remote entry (symlinks are described by their target).
FileKind fileKindOf(RemoteFileInfo entry) {
  if (entry.isDirectory) return const FileKind(FileKindGroup.folder);
  return switch (entry.kind) {
    RemoteEntryKind.other => const FileKind(FileKindGroup.special),
    RemoteEntryKind.symlink when entry.linkTargetKind == null => const FileKind(FileKindGroup.symlink),
    _ => fileKindForName(entry.name, executable: entry.isExecutable),
  };
}

/// Kind column text: symlinks read "Symlink → PHP".
String fileKindLabel(AppLocalizations l, RemoteFileInfo entry) {
  final kind = fileKindOf(entry);
  if (entry.isSymlink && kind.group != FileKindGroup.symlink) return l.sftpKindSymlinkTo(kind.label(l));
  return kind.label(l);
}
