import 'dart:convert';

import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/sftp/file_kind.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/widgets/numbered_text_preview.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// True if [bytes] look like text: no NUL in the first 8 KiB and (almost)
/// valid UTF-8.
bool looksLikeText(Uint8List bytes) {
  final head = bytes.length > 8192 ? Uint8List.sublistView(bytes, 0, 8192) : bytes;
  if (head.contains(0)) return false;
  final decoded = utf8.decode(head, allowMalformed: true);
  final bad = '�'.allMatches(decoded).length;
  return bad <= 2 || bad < decoded.length / 50;
}

/// Quick Look: text/code (monospace, first 256 KiB) and images, read into
/// memory through the service — never written to disk.
Future<void> showQuickLook(
  BuildContext context,
  RemoteFileInfo entry, {
  void Function(RemoteFileInfo entry)? onOpenInEditor,
}) => showDialog<void>(
  context: context,
  builder: (_) => QuickLookDialog(entry: entry, onOpenInEditor: onOpenInEditor),
);

sealed class _Preview {
  const _Preview();
}

final class _TextPreview extends _Preview {
  const _TextPreview(this.text, this.shown, this.total);

  final String text;
  final int shown;
  final int total;
}

final class _ImagePreview extends _Preview {
  const _ImagePreview(this.bytes);

  final Uint8List bytes;
}

final class _NoPreview extends _Preview {
  const _NoPreview(this.message);

  final String message;
}

class QuickLookDialog extends ConsumerStatefulWidget {
  const QuickLookDialog({required this.entry, super.key, this.onOpenInEditor});

  final RemoteFileInfo entry;
  final void Function(RemoteFileInfo entry)? onOpenInEditor;

  @override
  ConsumerState<QuickLookDialog> createState() => _QuickLookDialogState();
}

class _QuickLookDialogState extends ConsumerState<QuickLookDialog> {
  late final Future<_Preview> _preview = _load();

  Future<_Preview> _load() async {
    final l10n = context.l10n;
    final e = widget.entry;
    final kind = fileKindOf(e);
    if (e.isDirectory) return _NoPreview(l10n.sftpQuickLookFolder);
    if (kind.isKnownBinary) return _NoPreview(l10n.sftpQuickLookNoPreview);
    final session = ref.read(sftpControllerProvider).session;
    if (session == null) return _NoPreview(l10n.sftpQuickLookNoPreview);
    final service = ref.read(sftpBrowserServiceProvider);
    try {
      if (kind.isImage) {
        const cap = SftpBrowserService.maxImagePreviewBytes;
        if (e.size > cap) return _NoPreview(l10n.sftpQuickLookTooLarge(formatBytes(l10n, e.size)));
        final preview = await service.readPreview(session, e.path, maxBytes: cap);
        return _ImagePreview(preview.bytes);
      }
      final preview = await service.readPreview(session, e.path);
      if (!kind.isTextual && !looksLikeText(preview.bytes)) return _NoPreview(l10n.sftpQuickLookNoPreview);
      return _TextPreview(utf8.decode(preview.bytes, allowMalformed: true), preview.bytes.length, preview.totalSize);
    } on AppException catch (error) {
      return _NoPreview(errorMessage(l10n, error));
    }
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is KeyDownEvent &&
        (event.logicalKey == LogicalKeyboardKey.space || event.logicalKey == LogicalKeyboardKey.escape)) {
      Navigator.of(context).pop();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final e = widget.entry;
    final kind = fileKindOf(e);
    final size = MediaQuery.sizeOf(context);
    final compact = size.width < 600;
    return Dialog(
      key: const ValueKey('sftp-quicklook'),
      insetPadding: EdgeInsets.all(compact ? 12 : 32),
      child: Focus(
        autofocus: true,
        onKeyEvent: _onKey,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: 820, maxHeight: size.height * 0.8, minWidth: compact ? 0 : 360),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 10, 8, 10),
                child: Row(
                  children: [
                    Icon(kind.icon, color: kind.color(theme.colorScheme)),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(e.name, style: theme.textTheme.titleSmall, overflow: TextOverflow.ellipsis),
                          Text(
                            [if (!e.isDirectory) formatBytes(l10n, e.size), fileKindLabel(l10n, e)].join(' · '),
                            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                          ),
                        ],
                      ),
                    ),
                    if (widget.onOpenInEditor != null && !e.isDirectory)
                      TextButton.icon(
                        key: const ValueKey('sftp-quicklook-edit'),
                        onPressed: () {
                          Navigator.of(context).pop();
                          widget.onOpenInEditor!(e);
                        },
                        icon: const Icon(Icons.open_in_new, size: 16),
                        label: Text(l10n.sftpQuickLookOpenInEditor),
                      ),
                    IconButton(
                      tooltip: l10n.commonClose,
                      icon: const Icon(Icons.close),
                      onPressed: () => Navigator.of(context).pop(),
                    ),
                  ],
                ),
              ),
              const Divider(height: 1),
              Flexible(
                child: FutureBuilder<_Preview>(
                  future: _preview,
                  builder: (context, snapshot) => switch (snapshot.data) {
                    null => const SizedBox(height: 240, child: Center(child: CircularProgressIndicator())),
                    final _TextPreview p => _TextBody(preview: p),
                    final _ImagePreview p => Padding(
                      padding: const EdgeInsets.all(16),
                      child: InteractiveViewer(
                        child: Image.memory(
                          p.bytes,
                          key: const ValueKey('sftp-quicklook-image'),
                          fit: BoxFit.contain,
                          gaplessPlayback: true,
                          errorBuilder: (context, _, _) => _Message(l10n.sftpQuickLookImageError),
                        ),
                      ),
                    ),
                    final _NoPreview p => _Message(p.message),
                  },
                ),
              ),
              const Divider(height: 1),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
                child: Row(
                  children: [
                    Icon(Icons.memory, size: 14, color: theme.colorScheme.onSurfaceVariant),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        l10n.sftpQuickLookMemoryNote,
                        style: theme.textTheme.labelSmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _Message extends StatelessWidget {
  const _Message(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SizedBox(
      height: 200,
      child: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            text,
            key: const ValueKey('sftp-quicklook-message'),
            textAlign: TextAlign.center,
            style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        ),
      ),
    );
  }
}

class _TextBody extends StatelessWidget {
  const _TextBody({required this.preview});

  final _TextPreview preview;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final truncated = preview.shown < preview.total;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Flexible(child: NumberedTextPreview(text: preview.text)),
        if (truncated)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 6, 16, 0),
            child: Text(
              l10n.sftpQuickLookTruncated(formatBytes(l10n, preview.shown), formatBytes(l10n, preview.total)),
              key: const ValueKey('sftp-quicklook-truncated'),
              style: theme.textTheme.labelSmall?.copyWith(color: theme.colorScheme.tertiary),
            ),
          ),
      ],
    );
  }
}
