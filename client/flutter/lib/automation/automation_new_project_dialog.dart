import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

typedef NewAutomationProject = ({String name, String? directory, String? role});

class AutomationNewProjectDialog extends StatefulWidget {
  const AutomationNewProjectDialog({super.key, required this.chooseDirectory});

  final Future<String?> Function() chooseDirectory;

  @override
  State<AutomationNewProjectDialog> createState() => _AutomationNewProjectDialogState();
}

class _AutomationNewProjectDialogState extends State<AutomationNewProjectDialog> {
  final _name = TextEditingController();
  final _role = TextEditingController();
  String? _directory;
  var _withRole = false;
  var _choosing = false;
  var _directoryError = false;

  @override
  void dispose() {
    _name.dispose();
    _role.dispose();
    super.dispose();
  }

  Future<void> _choose() async {
    setState(() {
      _choosing = true;
      _directoryError = false;
    });
    try {
      final path = await widget.chooseDirectory();
      if (mounted && path != null) setState(() => _directory = path);
    } catch (_) {
      if (mounted) setState(() => _directoryError = true);
    } finally {
      if (mounted) setState(() => _choosing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final role = _role.text.trim();
    final validRole = RegExp(r'^[a-z][a-z0-9_]{0,63}$').hasMatch(role);
    return AlertDialog(
      title: Text(l.automationNewProject),
      content: SizedBox(
        width: 520,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                key: const ValueKey('automation-new-project-name'),
                autofocus: true,
                controller: _name,
                inputFormatters: [LengthLimitingTextInputFormatter(120)],
                decoration: InputDecoration(labelText: l.automationNewProjectName),
                onChanged: (_) => setState(() {}),
              ),
              const SizedBox(height: 12),
              DropdownButtonFormField<bool>(
                key: const ValueKey('automation-new-project-template'),
                initialValue: _withRole,
                decoration: InputDecoration(labelText: l.automationNewProjectType),
                items: [
                  DropdownMenuItem(value: false, child: Text(l.automationProjectPlaybook)),
                  DropdownMenuItem(value: true, child: Text(l.automationProjectRole)),
                ],
                onChanged: (value) {
                  if (value != null) setState(() => _withRole = value);
                },
              ),
              if (_withRole) ...[
                const SizedBox(height: 12),
                Text(l.automationProjectRoleHelp),
                const SizedBox(height: 8),
                TextField(
                  key: const ValueKey('automation-new-project-role'),
                  controller: _role,
                  inputFormatters: [LengthLimitingTextInputFormatter(64)],
                  decoration: InputDecoration(
                    labelText: l.automationProjectRoleName,
                    helperText: l.automationRoleNameHint,
                    helperMaxLines: 2,
                  ),
                  onChanged: (_) => setState(() {}),
                ),
              ],
              const SizedBox(height: 12),
              SelectableText(_directory ?? l.automationProjectProfileFolder),
              Wrap(
                children: [
                  TextButton(onPressed: _choosing ? null : _choose, child: Text(l.automationProjectChooseFolder)),
                  if (_directory != null)
                    TextButton(
                      onPressed: _choosing ? null : () => setState(() => _directory = null),
                      child: Text(l.automationProjectUseProfile),
                    ),
                ],
              ),
              if (_directoryError) Text(l.automationProjectChooseFolderError),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
        FilledButton(
          onPressed: _choosing || _name.text.trim().isEmpty || (_withRole && !validRole)
              ? null
              : () => Navigator.pop<NewAutomationProject>(context, (
                  name: _name.text.trim(),
                  directory: _directory,
                  role: _withRole ? role : null,
                )),
          child: Text(l.commonCreate),
        ),
      ],
    );
  }
}
