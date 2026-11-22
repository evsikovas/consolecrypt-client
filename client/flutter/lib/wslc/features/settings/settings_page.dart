import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:material_ui/material_ui.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key, required this.controller});
  final DeskController controller;
  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  late final path = TextEditingController(text: widget.controller.cliPath ?? '');
  @override
  void dispose() {
    path.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final runtime = c.runtime;
    const productName = 'WSLc';
    return ListView(
      children: [
        Panel(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(context.l10n.wslcWSLcConnection, style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 12),
              Text(context.l10n.wslcEnterThePathToTheInstalledWslcExe),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey('wslc-cli-path'),
                controller: path,
                decoration: InputDecoration(labelText: context.l10n.wslcPathToWslcExe),
              ),
              const SizedBox(height: 12),
              FilledButton.icon(
                onPressed: c.connecting ? null : () => c.connect(path.text.trim().isEmpty ? null : path.text.trim()),
                icon: const Icon(Icons.refresh_rounded),
                label: Text(context.l10n.dbTestConnection),
              ),
              if (runtime != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(
                    '$productName ${runtime.cliVersion} · ${runtime.readable ? context.l10n.wslcAvailable : context.l10n.wslcUnavailable}',
                  ),
                ),
              const SizedBox(height: 20),
              DropdownButtonFormField<int>(
                initialValue: c.refreshSeconds,
                decoration: InputDecoration(labelText: context.l10n.wslcRefreshData),
                items: [
                  for (final seconds in const [2, 5, 10])
                    DropdownMenuItem(value: seconds, child: Text(context.l10n.wslcEveryS(seconds))),
                ],
                onChanged: (value) {
                  if (value != null) c.setRefreshSeconds(value);
                },
              ),
              const SizedBox(height: 16),
              Text(context.l10n.wslcSavedRunTemplates(c.templates.length)),
              const SizedBox(height: 8),
              Text(context.l10n.wslcTemplatesAreStoredInThisProfileSEncrypted),
            ],
          ),
        ),
      ],
    );
  }
}
