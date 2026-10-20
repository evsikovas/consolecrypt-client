import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Schema suggestions are text edits only. The existing server dry-run,
/// resource identity check and write gate still apply to the resulting source.
Future<void> showKubeSchemaFields(
  BuildContext context, {
  required KubeSession session,
  required KubeResourceKind kind,
  required KubeManifestService service,
  required TextEditingController editor,
  required VoidCallback onChanged,
}) async {
  final original = editor.value;
  final scope = ProviderScope.containerOf(context, listen: false).read(kubeScopeProvider);
  final container = ProviderScope.containerOf(context, listen: false);
  final field = await showAppDialog<KubeSchemaField>(
    context,
    secure: true,
    builder: (_) => _SchemaFields(session: session, kind: kind, service: service),
  );
  if (!context.mounted ||
      field == null ||
      !identical(scope, container.read(kubeScopeProvider)) ||
      !scope.unlocked ||
      editor.text != original.text) {
    return;
  }
  final source = original.text;
  final selection = original.selection;
  final start = selection.isValid ? selection.start.clamp(0, source.length) : source.length;
  final end = selection.isValid ? selection.end.clamp(start, source.length) : source.length;
  final token = source.trimLeft().startsWith('{') ? '${jsonEncode(field.name)}: ' : '${field.name}: ';
  if (source.length + token.length > 2 * 1024 * 1024) return;
  editor.value = TextEditingValue(
    text: source.replaceRange(start, end, token),
    selection: TextSelection.collapsed(offset: start + token.length),
  );
  onChanged();
}

final class KubeSchemaField {
  const KubeSchemaField({required this.name, required this.path, required this.schema, this.required = false});
  final String name;
  final List<String> path;
  final KubeJson schema;
  final bool required;
}

/// Resolves only local OpenAPI references and caps traversal to avoid recursive
/// schema loops or an unbounded table from a large CRD.
List<KubeSchemaField> kubeSchemaFields(KubeJson document, List<String> path) {
  final definitions = document['definitions'] as KubeJson? ?? const {};
  var budget = 8192;
  KubeJson resolve(KubeJson node, [Set<String>? visited, int depth = 0]) {
    if (depth > 24 || budget-- <= 0) return const {};
    final seen = visited ?? <String>{};
    final reference = node[r'$ref'];
    if (reference is String && reference.startsWith('#/definitions/') && seen.length < 16 && seen.add(reference)) {
      final target = definitions[reference.substring(14)];
      if (target is KubeJson) return resolve(target, seen, depth + 1);
    }
    final properties = <String, dynamic>{...?node['properties'] as KubeJson?};
    final required = <String>{...(node['required'] as List? ?? const []).whereType<String>()};
    for (final part in (node['allOf'] as List? ?? const []).take(32).whereType<KubeJson>()) {
      final merged = resolve(part, {...seen}, depth + 1);
      properties.addAll(merged['properties'] as KubeJson? ?? const {});
      required.addAll((merged['required'] as List? ?? const []).whereType<String>());
    }
    return {...node, 'properties': properties, 'required': required.toList()};
  }

  var current = resolve(document['schema'] as KubeJson? ?? const {});
  for (final segment in path.take(24)) {
    final properties = current['properties'] as KubeJson? ?? const {};
    final child = properties[segment];
    if (child is! KubeJson) return const [];
    current = resolve(child);
    if (current['type'] == 'array' && current['items'] is KubeJson) {
      current = resolve(current['items'] as KubeJson);
    }
  }
  final required = (current['required'] as List? ?? const []).whereType<String>().toSet();
  final properties = current['properties'] as KubeJson? ?? const {};
  return properties.entries
      .take(1000)
      .where((e) => e.value is KubeJson)
      .map(
        (entry) => KubeSchemaField(
          name: entry.key,
          path: [...path, entry.key],
          schema: resolve(entry.value as KubeJson),
          required: required.contains(entry.key),
        ),
      )
      .toList()
    ..sort((a, b) => a.name.compareTo(b.name));
}

class _SchemaFields extends ConsumerStatefulWidget {
  const _SchemaFields({required this.session, required this.kind, required this.service});
  final KubeSession session;
  final KubeResourceKind kind;
  final KubeManifestService service;
  @override
  ConsumerState<_SchemaFields> createState() => _SchemaFieldsState();
}

class _SchemaFieldsState extends ConsumerState<_SchemaFields> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  KubeJson? _document;
  final _path = <String>[];
  String _query = '';
  bool _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final document = await widget.service.resourceSchema(widget.session.id, widget.kind);
      if (_current) setState(() => _document = document);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  @override
  void dispose() {
    _document = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _document = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final fields = _document == null ? const <KubeSchemaField>[] : kubeSchemaFields(_document!, _path);
    final visible = fields
        .where((f) => '${f.name} ${f.schema['description'] ?? ''}'.toLowerCase().contains(_query))
        .toList();
    return GlassDialog(
      key: const ValueKey('kube-schema-fields'),
      title: l.kubeSchemaFields,
      width: 800,
      content: SizedBox(
        height: 500,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.kubeSchemaHelp),
            TextField(
              key: const ValueKey('kube-schema-search'),
              decoration: InputDecoration(labelText: l.commonSearch),
              onChanged: (value) => setState(() => _query = value.toLowerCase()),
            ),
            Row(
              children: [
                GlassIconButton(
                  tooltip: l.kubeSchemaParent,
                  icon: Icons.arrow_upward,
                  onPressed: _path.isEmpty ? null : () => setState(() => _path.removeLast()),
                ),
                Expanded(child: Text(_path.isEmpty ? l.kubeSchemaRoot : _path.join('.'))),
              ],
            ),
            if (_failed) Text(l.kubeSchemaUnavailable),
            if (_document == null && !_failed) const LinearProgressIndicator(),
            if (_document != null && visible.isEmpty) Text(l.kubeSchemaNoFields),
            Expanded(
              child: ListView.builder(
                itemCount: visible.length,
                itemBuilder: (context, index) {
                  final field = visible[index];
                  final schema = field.schema;
                  final description = schema['description'] as String? ?? '';
                  final nested =
                      (schema['properties'] as KubeJson?)?.isNotEmpty == true ||
                      schema['type'] == 'array' ||
                      schema[r'$ref'] != null;
                  return ListTile(
                    key: ValueKey('kube-schema-field-${field.name}'),
                    isThreeLine: true,
                    title: Text(field.name),
                    subtitle: Text(
                      [
                        schema['type']?.toString() ?? 'object',
                        if (field.required) l.kubeSchemaRequired,
                        if (schema['enum'] is List) (schema['enum'] as List).take(20).join(', '),
                        description.length > 2000 ? description.substring(0, 2000) : description,
                      ].join(' · '),
                      maxLines: 4,
                      overflow: TextOverflow.ellipsis,
                    ),
                    onTap: nested
                        ? () => setState(() {
                            _path.add(field.name);
                            _query = '';
                          })
                        : null,
                    trailing: GlassIconButton(
                      key: ValueKey('kube-schema-insert-${field.name}'),
                      tooltip: l.kubeSchemaInsert,
                      icon: Icons.input,
                      onPressed: _current ? () => closeDialog(context, field) : null,
                    ),
                  );
                },
              ),
            ),
          ],
        ),
      ),
      primaryAction: GlassButton(label: l.commonCancel, onPressed: () => closeDialog<KubeSchemaField>(context)),
    );
  }
}
