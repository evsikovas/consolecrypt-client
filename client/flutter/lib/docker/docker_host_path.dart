import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart' show revealLabel;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';
import 'package:url_launcher/url_launcher.dart';

/// Whether a host path of [session] can be browsed. Bind sources always
/// can; named volumes live in `/var/lib/docker/volumes`, which on macOS and
/// Windows is inside Docker's virtual machine when Docker runs locally.
bool dockerHostPathReachable(DockerSessionInfo session, {required bool volume}) =>
    !volume || session.hostId != DockerHost.localId || AppPlatform.isLinux;

/// A bind source as reported by a local Docker Desktop engine → the path on
/// this computer (`/host_mnt/Users/…` on macOS, `/run/desktop/mnt/host/c/…`
/// on Windows).
String dockerLocalPath(String source, {bool? windows}) {
  const mac = '/host_mnt/'; // l10n-ignore: Docker Desktop path prefix
  const win = '/run/desktop/mnt/host/'; // l10n-ignore: Docker Desktop path prefix
  if (source.startsWith(mac)) return source.substring(mac.length - 1);
  if ((windows ?? AppPlatform.isWindows) && source.startsWith(win)) {
    final rest = source.substring(win.length);
    final slash = rest.indexOf('/');
    final drive = slash < 0 ? rest : rest.substring(0, slash);
    if (drive.length == 1) {
      final tail = slash < 0 ? '' : rest.substring(slash + 1).replaceAll('/', r'\');
      return '${drive.toUpperCase()}:\\$tail';
    }
  }
  return source;
}

/// Opens a Docker host path: over SSH in the SFTP browser of the same saved
/// host (a file opens its folder with the file selected); for Docker on
/// this computer in the system file manager.
Future<void> openDockerHostPath(BuildContext context, WidgetRef ref, DockerSessionInfo session, String path) async {
  if (session.hostId == DockerHost.localId) return _revealLocal(context, dockerLocalPath(path));
  final id = ObjectId(session.hostId);
  final host = ref.read(hostByIdProvider)[id] ?? await _loadHost(ref, id);
  if (!context.mounted) return;
  if (host == null) {
    showSnack(context, context.l10n.errorReasonHostNotFound, error: true);
    return;
  }
  // Show the SFTP section right away: it reports connecting / failures.
  final opening = ref.read(sftpControllerProvider.notifier).openHostPath(host, path);
  GoRouter.maybeOf(context)?.go(AppRoutes.sftp);
  await opening;
}

/// The host list may not be loaded yet when no other screen watches it
/// (an unlistened stream provider is paused): listen until it arrives.
Future<Host?> _loadHost(WidgetRef ref, ObjectId id) async {
  final subscription = ref.listenManual(hostsProvider.future, (_, _) {});
  try {
    final hosts = await subscription.read().timeout(const Duration(seconds: 10));
    return hosts.where((h) => h.id == id).firstOrNull;
  } on Object {
    return null;
  } finally {
    subscription.close();
  }
}

Future<void> _revealLocal(BuildContext context, String path) async {
  final l = context.l10n;
  var dir = path;
  if (FileSystemEntity.isFileSync(path)) {
    dir = File(path).parent.path;
  } else {
    try {
      await Directory(path).list().isEmpty;
    } on FileSystemException catch (e) {
      final code = e.osError?.errorCode;
      // ENOENT (ERROR_FILE/PATH_NOT_FOUND on Windows); EACCES (ERROR_ACCESS_DENIED).
      if (code == 2 || (AppPlatform.isWindows && code == 3)) {
        if (context.mounted) showSnack(context, l.errorReasonDirectoryNotFound(path), error: true);
        return;
      }
      if (code == 13 || (AppPlatform.isWindows && code == 5)) {
        if (context.mounted) showSnack(context, l.errorForbidden, error: true);
        return;
      }
      // Anything else (e.g. privacy prompts): let the file manager decide.
    }
  }
  final opened = await launchUrl(Uri.directory(dir, windows: AppPlatform.isWindows));
  if (!opened && context.mounted) showSnack(context, l.errorUnsupported, error: true);
}

/// A host path (bind source or volume mount point) as a link: opens it in
/// SFTP / the file manager, or — for a volume inside Docker's VM — stays
/// plain text with a tooltip saying why it cannot be opened.
class DockerHostPathLink extends ConsumerWidget {
  const DockerHostPathLink({
    required this.session,
    required this.path,
    required this.volume,
    this.label,
    this.style,
    super.key,
  });

  final DockerSessionInfo session;

  /// Path on the Docker host.
  final String path;

  /// A named volume (its mount point) rather than a bind mount.
  final bool volume;

  /// Text shown instead of [path] (e.g. the volume name).
  final String? label;
  final TextStyle? style;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final base = style ?? tokens.typography.mono.copyWith(fontSize: 12);
    final text = label ?? path;
    if (path.isEmpty || !dockerHostPathReachable(session, volume: volume)) {
      final plain = Text(text, key: ValueKey('docker-path-disabled-$path'), style: base);
      return path.isEmpty ? plain : Tooltip(message: l.dockerVolumeInVm, child: plain);
    }
    final action = session.hostId == DockerHost.localId ? revealLabel(l) : l.dockerOpenInSftp;
    return Align(
      alignment: AlignmentDirectional.centerStart,
      widthFactor: 1,
      child: Tooltip(
        message: text == path ? action : '$action\n$path',
        child: GlassInteractive(
          key: ValueKey('docker-path-$path'),
          semanticLabel: '$action: $text',
          onPressed: () => unawaited(openDockerHostPath(context, ref, session, path)),
          builder: (context, state) => GlassFocusRing(
            visible: state.focusVisible,
            shape: GlassRadii.shape(4),
            child: Text(
              text,
              style: base.copyWith(
                color: state.pressed ? tokens.palette.accent.withValues(alpha: .7) : tokens.palette.accent,
                decoration: state.hovered ? TextDecoration.underline : TextDecoration.none,
                decorationColor: tokens.palette.accent,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
