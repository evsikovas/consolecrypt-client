import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Selecting a saved manifest only inserts text into the reviewed editor. It
/// never evaluates shell templates, variable substitutions or embedded commands.
class KubeManifestSnippets extends ConsumerStatefulWidget {
  const KubeManifestSnippets({super.key});
  @override
  ConsumerState<KubeManifestSnippets> createState() => _KubeManifestSnippetsState();
}

class _KubeManifestSnippetsState extends ConsumerState<KubeManifestSnippets> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  String _query = '';
  bool get _current => _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, scope) {
      if (identical(scope, _scope)) return;
      _query = '';
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final saved = ref.watch(snippetsProvider);
    final snippets = (saved.value ?? const <Snippet>[])
        .where(
          (snippet) =>
              snippet.snippetType == SnippetType.kubectl &&
              snippet.template.length <= 2 * 1024 * 1024 &&
              (RegExp(r'^apiVersion\s*:', multiLine: true).hasMatch(snippet.template) ||
                  snippet.template.trimLeft().startsWith('{')) &&
              '${snippet.name} ${snippet.tags.join(' ')}'.toLowerCase().contains(_query),
        )
        .toList();
    return GlassDialog(
      title: l.kubeManifestSnippets,
      width: 640,
      content: SizedBox(
        height: 440,
        child: !_current
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.kubeManifestSnippetsHelp),
                  TextField(
                    decoration: InputDecoration(labelText: l.commonSearch),
                    onChanged: (value) => setState(() => _query = value.toLowerCase()),
                  ),
                  if (saved.isLoading) const LinearProgressIndicator(),
                  if (saved.hasError) Text(l.kubeRequestFailed),
                  if (snippets.isEmpty && !saved.isLoading) Text(l.kubeNoManifestSnippets),
                  Expanded(
                    child: ListView.builder(
                      itemCount: snippets.length,
                      itemBuilder: (_, index) {
                        final snippet = snippets[index];
                        return ListTile(
                          title: Text(snippet.name),
                          subtitle: Text(snippet.description, maxLines: 2, overflow: TextOverflow.ellipsis),
                          onTap: () {
                            if (_current) closeDialog(context, snippet.template);
                          },
                        );
                      },
                    ),
                  ),
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonCancel, onPressed: () => closeDialog<String>(context)),
    );
  }
}
