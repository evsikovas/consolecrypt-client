import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

/// Input collection only. These fields cannot create a panel draft. The parent
/// must obtain a typed review from the isolated resolver before prefill.
class MonitoringGrafanaConstants extends StatefulWidget {
  const MonitoringGrafanaConstants({super.key, required this.names, required this.onChanged});
  final List<String> names;
  final ValueChanged<Map<String, String>> onChanged;
  @override
  State<MonitoringGrafanaConstants> createState() => _MonitoringGrafanaConstantsState();
}

class _MonitoringGrafanaConstantsState extends State<MonitoringGrafanaConstants> {
  late final fields = {for (final name in widget.names) name: TextEditingController()};
  final acknowledged = <String>{};

  void publish() => widget.onChanged(
    Map.unmodifiable({
      for (final name in widget.names)
        if (acknowledged.contains(name)) name: fields[name]!.text,
    }),
  );

  @override
  void dispose() {
    for (final field in fields.values) {
      field.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.grafanaConstantsTitle, style: Theme.of(context).textTheme.titleSmall),
          Text(l.grafanaConstantsHint),
          const SizedBox(height: 8),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 240),
            child: ListView(
              key: const ValueKey('grafana-constant-fields'),
              shrinkWrap: true,
              children: [
                for (final name in widget.names)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Row(
                          children: [
                            Checkbox(
                              key: ValueKey('grafana-constant-use-$name'),
                              value: acknowledged.contains(name),
                              onChanged: (value) {
                                setState(() {
                                  value == true ? acknowledged.add(name) : acknowledged.remove(name);
                                });
                                publish();
                              },
                            ),
                            Expanded(child: Text(l.grafanaConstantUse(name))),
                          ],
                        ),
                        if (acknowledged.contains(name)) ...[
                          TextField(
                            key: ValueKey('grafana-constant-value-$name'),
                            controller: fields[name],
                            maxLength: 512,
                            autocorrect: false,
                            enableSuggestions: false,
                            decoration: InputDecoration(labelText: l.grafanaConstantValue),
                            onChanged: (_) {
                              setState(() {});
                              publish();
                            },
                          ),
                          if (fields[name]!.text.isEmpty) Text(l.grafanaConstantExplicitEmpty),
                        ],
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
