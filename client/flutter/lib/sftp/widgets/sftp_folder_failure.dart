import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Shown instead of the file list when the current folder could not be
/// listed — most often "permission denied" (e.g. `/var/lib/docker` for a
/// non-root SSH user) — so it never looks like an empty folder.
class SftpFolderFailure extends ConsumerWidget {
  const SftpFolderFailure({required this.error, required this.path, super.key});

  final AppException error;
  final String path;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final forbidden = error.code == AppErrorCode.forbidden;
    return EmptyState(
      key: ValueKey(forbidden ? 'sftp-list-forbidden' : 'sftp-list-failed'),
      icon: forbidden ? Icons.lock_outline_rounded : Icons.error_outline_rounded,
      title: errorMessage(l10n, error),
      message: path,
      compact: true,
      action: GlassButton(
        key: const ValueKey('sftp-list-retry'),
        label: l10n.commonRetry,
        icon: Icons.refresh_rounded,
        onPressed: () => unawaited(ref.read(sftpControllerProvider.notifier).refresh()),
      ),
    );
  }
}
