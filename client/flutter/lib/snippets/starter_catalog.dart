import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/starter_catalog_db.dart';

/// Bundled examples are imported only on explicit request. Once imported,
/// commands are ordinary E2EE vault objects; edits and deletions stay authoritative.
final class StarterSnippetPackage {
  const StarterSnippetPackage(this.id, this.name, this.description, this.snippets);
  final String id;
  final String name;
  final String description;
  final List<Snippet> snippets;

  List<Snippet> missingFrom(Iterable<Snippet> saved) {
    final imported = saved.map((s) => s.catalogId).toSet();
    return snippets.where((s) => !imported.contains(s.catalogId)).toList();
  }
}

/// Builds one catalog entry; `catalogId` is `consolecrypt.<id>.v1`.
typedef StarterEntry = Snippet Function(
  String id,
  String name,
  String package,
  SnippetType type,
  String command, {
  List<SnippetVariable> variables,
  RiskLevel risk,
  String? shell,
  String? description,
  List<String> tags,
});

/// The base starter set: Linux, Docker, Kubernetes and the database packages.
List<StarterSnippetPackage> starterSnippetPackages(AppLocalizations l) {
  final now = DateTime.now().toUtc();
  Snippet entry(
    String id,
    String name,
    String package,
    SnippetType type,
    String command, {
    List<SnippetVariable> variables = const [],
    RiskLevel risk = RiskLevel.readOnly,
    String? shell = 'sh',
    String? description,
    List<String> tags = const [],
  }) => Snippet(
    id: ObjectId.generate(),
    name: name,
    packageName: package,
    catalogId: 'consolecrypt.$id.v1',
    description: description ?? l.snippetStarterOrigin(package),
    snippetType: type,
    shell: shell,
    template: command,
    variables: variables,
    tags: tags,
    riskLevel: risk,
    source: SnippetSource.imported,
    createdAt: now,
    updatedAt: now,
  );
  const lines = SnippetVariable(name: 'lines', defaultValue: '100');
  const namespace = SnippetVariable(name: 'namespace', defaultValue: 'default');
  return [
    StarterSnippetPackage('linux', l.snippetPackLinux, l.snippetPackLinuxDescription, [
      entry('linux.uptime', l.snippetCatalogUptime, l.snippetPackLinux, SnippetType.shell, 'uptime'),
      entry('linux.disk', l.snippetCatalogDisk, l.snippetPackLinux, SnippetType.shell, 'df -h'),
      entry('linux.memory', l.snippetCatalogMemory, l.snippetPackLinux, SnippetType.shell, 'free -h'),
      entry('linux.ports', l.snippetCatalogPorts, l.snippetPackLinux, SnippetType.shell, 'ss -lntup'),
      entry(
        'linux.journal',
        l.snippetCatalogJournal,
        l.snippetPackLinux,
        SnippetType.shell,
        'journalctl -u {{service}} -n {{lines}} --no-pager',
        variables: [
          const SnippetVariable(name: 'service', defaultValue: 'sshd'),
          lines,
        ],
      ),
    ]),
    StarterSnippetPackage('docker', l.snippetPackDocker, l.snippetPackDockerDescription, [
      entry(
        'docker.containers',
        l.snippetCatalogContainers,
        l.snippetPackDocker,
        SnippetType.docker,
        'docker ps -a', // l10n-ignore: shell command
      ),
      entry(
        'docker.compose',
        l.snippetCatalogCompose,
        l.snippetPackDocker,
        SnippetType.docker,
        'docker compose ps', // l10n-ignore: shell command
      ),
      entry(
        'docker.stats',
        l.snippetCatalogStats,
        l.snippetPackDocker,
        SnippetType.docker,
        'docker stats --no-stream', // l10n-ignore: shell command
      ),
      entry(
        'docker.logs',
        l.snippetCatalogDockerLogs,
        l.snippetPackDocker,
        SnippetType.docker,
        'docker logs --tail {{lines}} {{container}}', // l10n-ignore: shell command
        variables: [
          lines,
          const SnippetVariable(name: 'container'),
        ],
      ),
      entry(
        'docker.disk',
        l.snippetCatalogDockerDisk,
        l.snippetPackDocker,
        SnippetType.docker,
        'docker system df', // l10n-ignore: shell command
      ),
    ]),
    StarterSnippetPackage('kubernetes', l.snippetPackKubernetes, l.snippetPackKubernetesDescription, [
      entry(
        'kubernetes.nodes',
        l.snippetCatalogNodes,
        l.snippetPackKubernetes,
        SnippetType.kubectl,
        'kubectl get nodes -o wide', // l10n-ignore: shell command
      ),
      entry(
        'kubernetes.pods',
        l.snippetCatalogPods,
        l.snippetPackKubernetes,
        SnippetType.kubectl,
        'kubectl get pods -n {{namespace}} -o wide', // l10n-ignore: shell command
        variables: [namespace],
      ),
      entry(
        'kubernetes.services',
        l.snippetCatalogServices,
        l.snippetPackKubernetes,
        SnippetType.kubectl,
        'kubectl get svc -n {{namespace}}', // l10n-ignore: shell command
        variables: [namespace],
      ),
      entry(
        'kubernetes.logs',
        l.snippetCatalogPodLogs,
        l.snippetPackKubernetes,
        SnippetType.kubectl,
        'kubectl logs -n {{namespace}} {{pod}} --tail={{lines}}', // l10n-ignore: shell command
        variables: [
          namespace,
          const SnippetVariable(name: 'pod'),
          lines,
        ],
      ),
      entry(
        'kubernetes.events',
        l.snippetCatalogEvents,
        l.snippetPackKubernetes,
        SnippetType.kubectl,
        'kubectl get events -n {{namespace}} --sort-by=.lastTimestamp', // l10n-ignore: shell command
        variables: [namespace],
      ),
    ]),
    ...databaseStarterPackages(l, entry),
  ];
}
