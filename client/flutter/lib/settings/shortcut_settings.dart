import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Remappable app shortcuts (desktop): change, turn off, restore one or all.
class ShortcutSettingsSection extends ConsumerWidget {
  const ShortcutSettingsSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (AppPlatform.isMobile) return const SizedBox.shrink();
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final commands = ref.watch(appCommandsProvider);
    final overrides = ref.watch(localSettingsProvider.select((s) => s.value?.shortcutOverrides)) ?? const {};
    final settings = ref.read(settingsServiceProvider);
    Future<void> save(Map<String, String> next) =>
        settings.updateLocal(settings.currentLocal.copyWith(shortcutOverrides: Map.unmodifiable(next)));
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: SectionCard(
        key: const ValueKey('settings-shortcuts'),
        title: l.settingsShortcutsTitle,
        icon: Icons.keyboard_alt_outlined,
        subtitle: l.settingsShortcutsSubtitle,
        trailing: GlassButton.plain(
          key: const ValueKey('shortcuts-reset-all'),
          icon: Icons.restart_alt_rounded,
          label: l.settingsShortcutsReset,
          onPressed: overrides.isEmpty ? null : () => save(const {}),
        ),
        child: Column(
          children: [
            for (final command in commands)
              Builder(
                builder: (context) {
                  final factory = commandFor(command.id).binding!;
                  final changed = overrides.containsKey(command.id.name);
                  return Padding(
                    padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
                    child: Row(
                      children: [
                        Expanded(
                          child: Text(
                            command.label(l),
                            style: tokens.typography.body.copyWith(color: tokens.palette.label),
                          ),
                        ),
                        if (command.binding?.reserved ?? false)
                          Padding(
                            padding: const EdgeInsets.only(right: GlassSpacing.s8),
                            child: Tooltip(
                              message: l.settingsShortcutReservedBadge,
                              child: Icon(Icons.warning_amber_rounded, size: 18, color: tokens.palette.warning),
                            ),
                          ),
                        _KeyCap(
                          key: ValueKey('shortcut-${command.id.name}'),
                          label: command.binding?.label ?? l.settingsShortcutOff,
                          changed: changed,
                        ),
                        const SizedBox(width: GlassSpacing.s8),
                        GlassButton.plain(
                          key: ValueKey('shortcut-change-${command.id.name}'),
                          label: l.settingsShortcutChange,
                          onPressed: () async {
                            final result = await _capture(context, ref, command, commands);
                            if (result == null) return;
                            final next = {...overrides};
                            final binding = result.binding;
                            // Free the combination from the command that had it.
                            for (final other in commands) {
                              if (other.id != command.id && binding != null && other.binding == binding) {
                                next[other.id.name] = '';
                              }
                            }
                            if (binding == factory) {
                              next.remove(command.id.name);
                            } else {
                              next[command.id.name] = binding?.encode() ?? '';
                            }
                            await save(next);
                          },
                        ),
                        SizedBox(
                          width: GlassSizes.toolbarButton,
                          child: changed
                              ? GlassIconButton(
                                  key: ValueKey('shortcut-reset-${command.id.name}'),
                                  icon: Icons.undo_rounded,
                                  tooltip: l.settingsShortcutResetOne(factory.label),
                                  style: GlassIconButtonStyle.plain,
                                  onPressed: () => save({...overrides}..remove(command.id.name)),
                                )
                              : null,
                        ),
                      ],
                    ),
                  );
                },
              ),
          ],
        ),
      ),
    );
  }

  static Future<({ShortcutBinding? binding})?> _capture(
    BuildContext context,
    WidgetRef ref,
    AppCommand command,
    List<AppCommand> commands,
  ) async {
    final capture = ref.read(shortcutCaptureProvider.notifier);
    capture.active = true;
    try {
      return await showAppDialog<({ShortcutBinding? binding})>(
        context,
        builder: (_) => _CaptureDialog(command: command, commands: commands),
      );
    } finally {
      capture.active = false;
    }
  }
}

class _KeyCap extends StatelessWidget {
  const _KeyCap({required this.label, required this.changed, super.key});
  final String label;
  final bool changed;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Container(
      constraints: const BoxConstraints(minWidth: 72),
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
      decoration: ShapeDecoration(
        color: changed ? tokens.palette.accent.withValues(alpha: .14) : tokens.surfaces.inset,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6)),
      ),
      child: Text(
        label,
        textAlign: TextAlign.center,
        style: tokens.typography.mono.copyWith(
          color: changed ? tokens.palette.accent : tokens.palette.label,
          fontSize: 13,
        ),
      ),
    );
  }
}

class _CaptureDialog extends StatefulWidget {
  const _CaptureDialog({required this.command, required this.commands});
  final AppCommand command;
  final List<AppCommand> commands;
  @override
  State<_CaptureDialog> createState() => _CaptureDialogState();
}

class _CaptureDialogState extends State<_CaptureDialog> {
  ShortcutBinding? _binding;

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.handled;
    final binding = ShortcutBinding.fromKey(event.logicalKey);
    if (event.logicalKey == LogicalKeyboardKey.escape && !binding.primary && !binding.alt) {
      closeDialog<({ShortcutBinding? binding})>(context);
      return KeyEventResult.handled;
    }
    if (binding.valid) setState(() => _binding = binding);
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final binding = _binding;
    final conflict = binding == null
        ? null
        : widget.commands.where((c) => c.id != widget.command.id && c.binding == binding).firstOrNull;
    return Focus(
      autofocus: true,
      onKeyEvent: _onKey,
      child: GlassDialog(
        key: const ValueKey('shortcut-capture'),
        title: l.settingsShortcutCaptureTitle(widget.command.label(l)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.settingsShortcutCaptureHint, style: TextStyle(color: tokens.secondaryLabel)),
            const SizedBox(height: GlassSpacing.s16),
            Center(
              child: Text(
                binding?.label ?? widget.command.binding?.label ?? '…',
                key: const ValueKey('shortcut-capture-value'),
                style: tokens.typography.title2.copyWith(
                  color: binding == null ? tokens.secondaryLabel : tokens.palette.label,
                ),
              ),
            ),
            const SizedBox(height: GlassSpacing.s12),
            if (conflict != null)
              Text(
                l.settingsShortcutConflict(conflict.label(l)),
                key: const ValueKey('shortcut-capture-conflict'),
                style: TextStyle(color: tokens.palette.danger),
              ),
            if (binding?.reserved ?? false)
              Text(
                l.settingsShortcutReserved,
                key: const ValueKey('shortcut-capture-reserved'),
                style: TextStyle(color: tokens.palette.warning),
              ),
          ],
        ),
        leadingAction: GlassButton.plain(
          key: const ValueKey('shortcut-capture-disable'),
          label: l.settingsShortcutDisable,
          onPressed: () => closeDialog(context, (binding: null)),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: () => closeDialog<({ShortcutBinding? binding})>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('shortcut-capture-assign'),
          label: l.settingsShortcutAssign,
          onPressed: binding == null ? null : () => closeDialog(context, (binding: binding)),
        ),
      ),
    );
  }
}
