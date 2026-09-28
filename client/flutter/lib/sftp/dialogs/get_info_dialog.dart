import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/file_kind.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Get Info: kind, size, location, dates, owner/group, link target and a
/// permissions editor (rwx checkboxes ⇄ octal) that applies `chmod`.
Future<void> showGetInfoDialog(BuildContext context, RemoteFileInfo entry) => showDialog<void>(
  context: context,
  builder: (_) => GetInfoDialog(entry: entry),
);

class GetInfoDialog extends ConsumerStatefulWidget {
  const GetInfoDialog({required this.entry, super.key});

  final RemoteFileInfo entry;

  @override
  ConsumerState<GetInfoDialog> createState() => _GetInfoDialogState();
}

class _GetInfoDialogState extends ConsumerState<GetInfoDialog> {
  late int _mode = widget.entry.permissions ?? 0;
  late final TextEditingController _octal = TextEditingController(text: formatOctalMode(_mode));
  bool _octalValid = true;
  bool _saving = false;

  // who: 0 owner, 1 group, 2 others; bit: 4 read, 2 write, 1 execute.
  static int _mask(int who, int bit) => bit << (6 - who * 3);

  bool get _changed => _mode != (widget.entry.permissions ?? 0);

  @override
  void dispose() {
    _octal.dispose();
    super.dispose();
  }

  void _setBit(int who, int bit, bool on) {
    setState(() {
      _mode = on ? _mode | _mask(who, bit) : _mode & ~_mask(who, bit);
      _octal.text = formatOctalMode(_mode);
      _octalValid = true;
    });
  }

  void _onOctal(String text) {
    final parsed = parseOctalMode(text);
    setState(() {
      _octalValid = parsed != null;
      if (parsed != null) _mode = parsed;
    });
  }

  Future<void> _apply() async {
    setState(() => _saving = true);
    final l10n = context.l10n;
    try {
      await ref.read(sftpControllerProvider.notifier).setPermissions(widget.entry.path, _mode);
      if (!mounted) return;
      Navigator.of(context).pop();
      showSnack(context, l10n.sftpInfoPermissionsSaved);
    } on AppException catch (e) {
      if (!mounted) return;
      setState(() => _saving = false);
      showSnack(context, errorMessage(l10n, e), error: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final theme = Theme.of(context);
    final e = widget.entry;
    final kind = fileKindOf(e);
    final mono = TextStyle(fontFamily: AppPlatform.monospaceFamily, fontFamilyFallback: AppPlatform.monospaceFallback);
    const labelWidth = 120.0;

    Widget value(String text, {TextStyle? style}) => SelectableText(text, style: style);

    final who = [l10n.sftpColumnOwner, l10n.sftpColumnGroup, l10n.sftpInfoOthers];
    const whoKeys = ['owner', 'group', 'others'];
    final bits = [(4, l10n.sftpInfoRead, 'r'), (2, l10n.sftpInfoWrite, 'w'), (1, l10n.sftpInfoExecute, 'x')];

    return AlertDialog(
      key: const ValueKey('sftp-info-dialog'),
      title: Row(
        children: [
          Icon(kind.icon, color: kind.color(theme.colorScheme)),
          const SizedBox(width: 10),
          Expanded(child: Text(l10n.sftpInfoTitle(e.name), overflow: TextOverflow.ellipsis)),
        ],
      ),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              LabeledValue(labelWidth: labelWidth, label: l10n.sftpColumnKind, value: value(fileKindLabel(l10n, e))),
              if (!e.isDirectory)
                LabeledValue(
                  labelWidth: labelWidth,
                  label: l10n.sftpColumnSize,
                  value: value(l10n.sftpInfoSizeBytes(formatBytes(l10n, e.size), formatCount(l10n, e.size))),
                ),
              LabeledValue(
                labelWidth: labelWidth,
                label: l10n.sftpInfoWhere,
                value: value(parentRemotePath(e.path), style: mono),
              ),
              if (e.isSymlink)
                LabeledValue(
                  labelWidth: labelWidth,
                  label: l10n.sftpInfoLinkTarget,
                  value: value(
                    e.linkTargetKind == null ? '${e.linkTarget ?? ''} (${l10n.sftpDanglingLink})' : e.linkTarget ?? '',
                    style: mono,
                  ),
                ),
              if (e.modifiedAt != null)
                LabeledValue(
                  labelWidth: labelWidth,
                  label: l10n.sftpColumnModified,
                  value: value(formatDateTime(l10n, e.modifiedAt!)),
                ),
              LabeledValue(
                labelWidth: labelWidth,
                label: l10n.sftpColumnOwner,
                value: value(_principal(e.owner, e.uid)),
              ),
              LabeledValue(
                labelWidth: labelWidth,
                label: l10n.sftpColumnGroup,
                value: value(_principal(e.group, e.gid)),
              ),
              const Divider(height: 24),
              Row(
                children: [
                  Expanded(child: Text(l10n.sftpColumnPermissions, style: theme.textTheme.titleSmall)),
                  Text(formatModeString(e.kind, _mode), key: const ValueKey('sftp-info-mode-string'), style: mono),
                ],
              ),
              if (e.isSymlink)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(l10n.sftpInfoSymlinkNote, style: theme.textTheme.bodySmall),
                ),
              const SizedBox(height: 8),
              Table(
                columnWidths: const {0: FlexColumnWidth(1.4)},
                defaultVerticalAlignment: TableCellVerticalAlignment.middle,
                children: [
                  TableRow(
                    children: [
                      const SizedBox.shrink(),
                      for (final (_, label, _) in bits)
                        Center(
                          child: Text(label, style: theme.textTheme.labelMedium, textAlign: TextAlign.center),
                        ),
                    ],
                  ),
                  for (var w = 0; w < 3; w++)
                    TableRow(
                      children: [
                        Text(who[w]),
                        for (final (bit, label, short) in bits)
                          Center(
                            child: Checkbox(
                              key: ValueKey('sftp-info-perm-${whoKeys[w]}-$short'),
                              semanticLabel: '${who[w]}: $label',
                              value: _mode & _mask(w, bit) != 0,
                              onChanged: _saving ? null : (v) => _setBit(w, bit, v ?? false),
                            ),
                          ),
                      ],
                    ),
                ],
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  SizedBox(
                    width: 150,
                    child: TextField(
                      key: const ValueKey('sftp-info-octal'),
                      controller: _octal,
                      enabled: !_saving,
                      style: mono,
                      maxLength: 4,
                      inputFormatters: [FilteringTextInputFormatter.allow(RegExp('[0-7]'))],
                      decoration: InputDecoration(
                        labelText: l10n.sftpInfoOctal,
                        counterText: '',
                        errorText: _octalValid ? null : l10n.sftpInfoOctalInvalid,
                        errorMaxLines: 2,
                      ),
                      onChanged: _onOctal,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonClose)),
        FilledButton(
          key: const ValueKey('sftp-info-apply'),
          onPressed: _changed && _octalValid && !_saving ? _apply : null,
          child: Text(l10n.sftpInfoApply),
        ),
      ],
    );
  }

  static String _principal(String? name, int? id) {
    if (name != null && id != null) return '$name ($id)';
    return name ?? (id?.toString() ?? '—');
  }
}
