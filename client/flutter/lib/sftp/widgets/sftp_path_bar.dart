import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/widgets/sftp_drag.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Breadcrumb path bar (chrome): host ▸ / ▸ each folder; click navigates,
/// right-click offers "Copy path", a click on the empty area or Cmd/Ctrl+L
/// turns it into an editable field with folder autocomplete; ▲ = parent.
/// Segments accept dragged rows (move there).
class SftpPathBar extends ConsumerStatefulWidget {
  const SftpPathBar({super.key, this.onEditingDone});

  /// Called after the editable field closes (focus goes back to the list).
  final VoidCallback? onEditingDone;

  @override
  ConsumerState<SftpPathBar> createState() => SftpPathBarState();
}

class SftpPathBarState extends ConsumerState<SftpPathBar> {
  bool _editing = false;
  String? _error;
  final TextEditingController _text = TextEditingController();
  final FocusNode _focus = FocusNode(debugLabel: 'sftp-path-field');
  final ScrollController _scroll = ScrollController();

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (!_focus.hasFocus && _editing) _stopEditing();
    });
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    _scroll.dispose();
    super.dispose();
  }

  /// Cmd/Ctrl+L.
  void startEditing() {
    final path = ref.read(sftpControllerProvider).remotePath;
    setState(() {
      _editing = true;
      _error = null;
      _text.value = TextEditingValue(
        text: path,
        selection: TextSelection(baseOffset: 0, extentOffset: path.length),
      );
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => _focus.requestFocus());
  }

  void _stopEditing() {
    if (!mounted) return;
    setState(() {
      _editing = false;
      _error = null;
    });
    widget.onEditingDone?.call();
  }

  Future<void> _submit(String value) async {
    final l10n = context.l10n;
    try {
      await ref.read(sftpControllerProvider.notifier).goToTypedPath(value);
      _stopEditing();
    } on AppException catch (e) {
      if (!mounted) return;
      setState(() => _error = errorMessage(l10n, e));
      _focus.requestFocus();
    }
  }

  Future<void> _segmentMenu(Offset global, String path) async {
    final l10n = context.l10n;
    final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
    final local = overlay.globalToLocal(global);
    final choice = await showMenu<int>(
      context: context,
      position: RelativeRect.fromLTRB(local.dx, local.dy, local.dx, local.dy),
      items: [
        PopupMenuItem(
          key: const ValueKey('sftp-crumb-copy-path'),
          value: 0,
          child: Row(
            children: [const Icon(Icons.content_copy, size: 16), const SizedBox(width: 8), Text(l10n.sftpCopyPath)],
          ),
        ),
      ],
    );
    if (choice == 0 && mounted) {
      await copyPlainWithNotice(context, ref, path, what: l10n.sftpCopyWhatPath);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final state = ref.watch(sftpControllerProvider);
    final controller = ref.read(sftpControllerProvider.notifier);
    final path = state.remotePath;
    final parts = path.split('/').where((p) => p.isNotEmpty).toList();
    final crumbs = <(String, String, bool)>[
      (state.host?.name ?? '', state.home, true),
      ('/', '/', false),
      for (var i = 0; i < parts.length; i++) (parts[i], '/${parts.sublist(0, i + 1).join('/')}', false),
    ];

    Widget body;
    if (_editing) {
      body = CallbackShortcuts(
        bindings: {const SingleActivator(LogicalKeyboardKey.escape): _stopEditing},
        child: RawAutocomplete<String>(
          textEditingController: _text,
          focusNode: _focus,
          optionsBuilder: (value) => controller.completePath(value.text),
          onSelected: (option) => unawaited(_submit(option)),
          fieldViewBuilder: (context, textController, focusNode, onSubmit) => TextField(
            key: const ValueKey('sftp-path-field'),
            controller: textController,
            focusNode: focusNode,
            autocorrect: false,
            enableSuggestions: false,
            style: TextStyle(
              fontFamily: AppPlatform.monospaceFamily,
              fontFamilyFallback: AppPlatform.monospaceFallback,
              fontSize: 13,
            ),
            decoration: InputDecoration(
              isDense: true,
              hintText: l10n.sftpPathHint,
              errorText: _error,
              contentPadding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
            ),
            // Keep focus on submit so an error can be corrected in place.
            onEditingComplete: () {},
            onSubmitted: (v) => unawaited(_submit(v)),
          ),
          optionsViewBuilder: (context, onSelected, options) => Align(
            alignment: AlignmentDirectional.topStart,
            child: Material(
              elevation: 4,
              borderRadius: BorderRadius.circular(8),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 240, maxWidth: 520),
                child: ListView(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  shrinkWrap: true,
                  children: [
                    for (final option in options)
                      ListTile(
                        key: ValueKey('sftp-path-option-$option'),
                        dense: true,
                        leading: const Icon(Icons.folder, size: 18, color: Color(0xFF3D8FE0)),
                        title: Text(option, overflow: TextOverflow.ellipsis),
                        onTap: () => onSelected(option),
                      ),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
    } else {
      body = GestureDetector(
        key: const ValueKey('sftp-path-bar'),
        behavior: HitTestBehavior.opaque,
        onTap: startEditing,
        child: Tooltip(
          message: l10n.sftpPathEditTooltip(AppPlatform.shortcutLabel(LogicalKeyboardKey.keyL)),
          waitDuration: const Duration(seconds: 1),
          child: Align(
            alignment: AlignmentDirectional.centerStart,
            child: SingleChildScrollView(
              controller: _scroll,
              scrollDirection: Axis.horizontal,
              reverse: true,
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (var i = 0; i < crumbs.length; i++) ...[
                    if (i > 0) Icon(Icons.chevron_right, size: 16, color: theme.colorScheme.outline),
                    _Crumb(
                      key: ValueKey(crumbs[i].$3 ? 'sftp-crumb-host' : 'sftp-crumb-${crumbs[i].$2}'),
                      label: crumbs[i].$1,
                      isHost: crumbs[i].$3,
                      current: !crumbs[i].$3 && crumbs[i].$2 == path,
                      onTap: () => unawaited(controller.navigateTo(crumbs[i].$2)),
                      onMenu: (global) => unawaited(_segmentMenu(global, crumbs[i].$2)),
                      onDrop: (data) => unawaited(_moveTo(data, crumbs[i].$2)),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
      );
    }

    return SizedBox(
      height: _editing && _error != null ? 64 : 38,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(8, 2, 4, 2),
        child: Row(
          children: [
            Expanded(child: body),
            IconButton(
              key: const ValueKey('sftp-up'),
              tooltip: l10n.sftpUp,
              icon: const Icon(Icons.arrow_upward, size: 18),
              onPressed: path == '/' ? null : () => unawaited(controller.goUp()),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _moveTo(SftpDragData data, String directory) async {
    final l10n = context.l10n;
    final moved = await runWithFeedback(
      context,
      () => ref.read(sftpControllerProvider.notifier).move(data.paths, directory),
    );
    if (moved != null && moved > 0 && mounted) showSnack(context, l10n.sftpMoved(moved, directory));
  }
}

class _Crumb extends StatefulWidget {
  const _Crumb({
    required this.label,
    required this.isHost,
    required this.current,
    required this.onTap,
    required this.onMenu,
    required this.onDrop,
    super.key,
  });

  final String label;
  final bool isHost;
  final bool current;
  final VoidCallback onTap;
  final ValueChanged<Offset> onMenu;
  final ValueChanged<SftpDragData> onDrop;

  @override
  State<_Crumb> createState() => _CrumbState();
}

class _CrumbState extends State<_Crumb> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return DragTarget<SftpDragData>(
      onWillAcceptWithDetails: (_) => true,
      onAcceptWithDetails: (d) => widget.onDrop(d.data),
      builder: (context, candidates, _) {
        final highlight = candidates.isNotEmpty || _hover;
        return MouseRegion(
          cursor: SystemMouseCursors.click,
          onEnter: (_) => setState(() => _hover = true),
          onExit: (_) => setState(() => _hover = false),
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: widget.onTap,
            onSecondaryTapUp: (d) => widget.onMenu(d.globalPosition),
            onLongPressStart: (d) => widget.onMenu(d.globalPosition),
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: candidates.isNotEmpty
                    ? theme.colorScheme.primary.withValues(alpha: 0.18)
                    : (highlight ? theme.colorScheme.onSurface.withValues(alpha: 0.06) : null),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (widget.isHost) ...[
                      Icon(Icons.dns_outlined, size: 14, color: theme.colorScheme.primary),
                      const SizedBox(width: 4),
                    ],
                    Text(
                      widget.label,
                      style: theme.textTheme.bodyMedium?.copyWith(
                        fontWeight: widget.current ? FontWeight.w600 : null,
                        color: widget.current ? theme.colorScheme.onSurface : theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        );
      },
    );
  }
}
