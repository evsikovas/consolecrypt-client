import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_compose_yaml.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_screen.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Records saves; can fail validation or writing on demand.
class _Docker extends MockDockerService {
  final saves = <({String grant, List<String> paths, bool apply, bool allowSharedDirs})>[];
  DockerComposeSaveResult? nextResult;
  Exception? projectError;

  @override
  Future<DockerComposeProject> composeProject(String sessionId, String project) {
    final error = projectError;
    if (error != null) throw error;
    return super.composeProject(sessionId, project);
  }

  @override
  Future<DockerComposeSaveResult> composeSave(
    String sessionId,
    String project,
    String grant,
    List<DockerComposeEdit> edits, {
    bool apply = true,
    bool allowSharedDirs = false,
  }) async {
    saves.add((grant: grant, paths: [for (final e in edits) e.path], apply: apply, allowSharedDirs: allowSharedDirs));
    final forced = nextResult;
    if (forced != null) {
      nextResult = null;
      for (final edit in edits) {
        edit.content.wipe();
      }
      return forced;
    }
    return super.composeSave(sessionId, project, grant, edits, apply: apply, allowSharedDirs: allowSharedDirs);
  }
}

Future<_Docker> _pump(WidgetTester tester, {_Docker? service, Size size = const Size(1400, 900)}) async {
  final docker = service ?? _Docker();
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        dockerServiceProvider.overrideWithValue(docker),
        dockerScopeProvider.overrideWithValue(const DockerScope('profile', true)),
      ],
      child: const MaterialApp(
        locale: Locale('en'),
        localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: DockerScreen()),
      ),
    ),
  );
  await settle(tester);
  await tapKey(tester, 'docker-connect-local');
  return docker;
}

String _editorText(WidgetTester tester) =>
    tester.widget<TextField>(find.byKey(const ValueKey('docker-compose-editor'))).controller!.text;

Future<void> _unlock(WidgetTester tester, [String passphrase = MockDockerService.demoPassphrase]) async {
  await tester.enterText(find.byKey(const ValueKey('docker-compose-passphrase')), passphrase);
  await tapKey(tester, 'docker-compose-unlock');
}

void main() {
  test('YAML lines are coloured by keys, strings, scalars and comments', () {
    const styles = DockerYamlStyles(
      key: TextStyle(color: Color(0xff000001)),
      string: TextStyle(color: Color(0xff000002)),
      comment: TextStyle(color: Color(0xff000003)),
      scalar: TextStyle(color: Color(0xff000004)),
    );
    String? colorOf(List<InlineSpan> spans, String text) => spans
        .whereType<TextSpan>()
        .where((s) => s.text == text)
        .map((s) => s.style?.color?.toARGB32().toRadixString(16))
        .firstOrNull;
    final key = dockerYamlLineSpans('    image: "nginx:1.27" # pinned', styles);
    expect(colorOf(key, 'image'), 'ff000001');
    expect(colorOf(key, '"nginx:1.27"'), 'ff000002');
    expect(colorOf(key, '# pinned'), 'ff000003');
    final scalar = dockerYamlLineSpans('  - replicas: 3', styles);
    expect(colorOf(scalar, 'replicas'), 'ff000001');
    expect(colorOf(scalar, '3'), 'ff000004');
    // A '#' inside quotes or a word is not a comment.
    final hash = dockerYamlLineSpans("  url: 'http://x/#a' b#c", styles);
    expect(hash.whereType<TextSpan>().any((s) => s.style?.color == styles.comment.color), isFalse);
    expect(colorOf(dockerYamlLineSpans('# whole line', styles), '# whole line'), 'ff000003');
    // Every character survives highlighting.
    const line = '  - "8080:80"   # web';
    expect(dockerYamlLineSpans(line, styles).whereType<TextSpan>().map((s) => s.text).join(), line);
  });

  testWidgets('the compose header menu runs project-wide actions with confirmation', (tester) async {
    final docker = await _pump(tester);
    // Right click on the header opens the same menu as "…".
    await tester.tapAt(
      tester.getCenter(find.byKey(const ValueKey('docker-group-title-shop'))),
      buttons: kSecondaryButton,
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('docker-compose-menu-viewYaml')), findsOneWidget);
    // Every container of shop runs: nothing to start.
    await tester.tap(find.byKey(const ValueKey('docker-compose-menu-startAll')));
    await settle(tester);
    expect(docker.actions, isEmpty);
    await tapKey(tester, 'docker-compose-menu-stopAll');
    expect(find.text('Stop all containers of shop?'), findsOneWidget);
    expect(find.text('4 containers are affected.'), findsOneWidget);
    await tapKey(tester, 'confirm-ok');
    expect(docker.actions.where((a) => a.$2 == DockerAction.stop), hasLength(4));
    expect(tester.widget<Text>(find.byKey(const ValueKey('docker-group-count-shop'))).data, '0/4 running');

    // Start all needs no confirmation and only touches stopped containers.
    await tapKey(tester, 'docker-compose-menu-button-monitoring');
    await tapKey(tester, 'docker-compose-menu-startAll');
    expect(find.byType(GlassDialog), findsNothing);
    expect(docker.actions.last.$2, DockerAction.start);
    expect(docker.actions.where((a) => a.$2 == DockerAction.start), hasLength(1));
    expect(tester.widget<Text>(find.byKey(const ValueKey('docker-group-count-monitoring'))).data, '2/2 running');

    // Standalone containers have no project menu.
    expect(find.byKey(const ValueKey('docker-compose-menu-button-')), findsNothing);
  });

  testWidgets('view YAML shows files as tabs with line numbers and file errors', (tester) async {
    await _pump(tester);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-viewYaml');
    expect(find.byKey(const ValueKey('docker-compose-dialog')), findsOneWidget);
    expect(find.text('Compose files: shop'), findsOneWidget);
    expect(find.textContaining('Project folder: /srv/shop'), findsOneWidget);
    expect(find.textContaining('Environment files: /srv/shop/.env'), findsOneWidget);
    final text = tester.widget<SelectableText>(find.byKey(const ValueKey('docker-compose-text')));
    expect(text.textSpan!.toPlainText(), startsWith('# Shop: web front'));
    expect(find.textContaining(RegExp(r'^ 1\n 2\n 3')), findsOneWidget);
    await tapKey(tester, 'docker-compose-file-1');
    expect(
      tester.widget<SelectableText>(find.byKey(const ValueKey('docker-compose-text'))).textSpan!.toPlainText(),
      contains('LOG_LEVEL: debug'),
    );
    // Copying goes through the secret clipboard (auto-cleared).
    await tapKey(tester, 'docker-compose-copy');
    expect(find.textContaining('compose.override.yaml'), findsWidgets);
    await tapKey(tester, 'docker-compose-close');
    expect(find.byKey(const ValueKey('docker-compose-dialog')), findsNothing);

    // A root-only file explains itself; the readable one is still shown.
    await tapKey(tester, 'docker-compose-menu-button-monitoring');
    await tapKey(tester, 'docker-compose-menu-viewYaml');
    expect(find.byKey(const ValueKey('docker-compose-text')), findsOneWidget);
    await tapKey(tester, 'docker-compose-file-1');
    expect(find.byKey(const ValueKey('docker-compose-file-error')), findsOneWidget);
    expect(find.textContaining('No permission to read this file'), findsOneWidget);
    await tapKey(tester, 'docker-compose-close');
  });

  testWidgets('editing asks for the vault passphrase, validates, saves and applies', (tester) async {
    final docker = await _pump(tester);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-editYaml');
    expect(find.byKey(const ValueKey('docker-compose-unlock-panel')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsNothing);
    await _unlock(tester, 'wrong');
    expect(find.byKey(const ValueKey('docker-compose-wrong-passphrase')), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsNothing);
    await _unlock(tester);
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsOneWidget);
    expect(_editorText(tester), startsWith('# Shop'));

    // Nothing changed: nothing to save.
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('docker-compose-save-apply'))).onPressed, isNull);

    // A tab in the indentation fails validation; nothing is written.
    final original = docker.composeFiles['/srv/shop/compose.yaml']!;
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), 'services:\n\tweb: {}\n');
    await tester.pump();
    await tapKey(tester, 'docker-compose-save-apply');
    expect(find.text('The configuration has errors. Nothing was saved.'), findsOneWidget);
    expect(find.textContaining('yaml: line 2: found a tab character'), findsOneWidget);
    expect(docker.composeFiles['/srv/shop/compose.yaml'], original);
    expect(docker.composeBackups, isEmpty);

    // A valid edit is saved with a backup and applied; the output streams in.
    const edited = 'services:\n  web:\n    image: nginx:1.28-alpine\n';
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), edited);
    await tester.pump();
    await tapKey(tester, 'docker-compose-save-apply');
    expect(docker.saves.last.paths, ['/srv/shop/compose.yaml']);
    expect(docker.saves.last.apply, isTrue);
    expect(docker.composeFiles['/srv/shop/compose.yaml'], edited);
    expect(docker.composeBackups, ['/srv/shop/compose.yaml.bak-20261021-091500']);
    expect(find.byKey(const ValueKey('docker-compose-result')), findsOneWidget);
    expect(find.text('/srv/shop/compose.yaml.bak-20261021-091500'), findsOneWidget);
    expect(find.text('Changes applied.'), findsOneWidget);
    expect(find.textContaining('Container shop-web-1  Started'), findsOneWidget);
    // Back to the read-only view with the saved text.
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsNothing);
    expect(
      tester.widget<SelectableText>(find.byKey(const ValueKey('docker-compose-text'))).textSpan!.toPlainText(),
      edited,
    );

    // Editing again reuses the grant (no second passphrase).
    await tapKey(tester, 'docker-compose-edit');
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsOneWidget);
    await tapKey(tester, 'docker-compose-cancel-edit');
    await tapKey(tester, 'docker-compose-close');
    expect(find.byKey(const ValueKey('docker-compose-dialog')), findsNothing);
  });

  testWidgets('save errors are explained and an expired grant asks again', (tester) async {
    final docker = await _pump(tester);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-viewYaml');
    await tapKey(tester, 'docker-compose-edit');
    await _unlock(tester);
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), 'services: {}\n');
    await tester.pump();

    docker.nextResult = const DockerComposeWriteFailed(
      '/srv/shop/compose.yaml',
      DockerComposeWriteError.permissionDenied,
    );
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.textContaining('No permission to write /srv/shop/compose.yaml'), findsOneWidget);
    docker.nextResult = const DockerComposeMissing();
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.textContaining('no compose command line tool'), findsOneWidget);
    docker.nextResult = const DockerComposeConflict('/srv/shop/compose.yaml');
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.textContaining('changed on the host after it was opened'), findsOneWidget);
    // Still editing, text kept.
    expect(_editorText(tester), 'services: {}\n');

    // The grant expired: the passphrase is asked again and the save retried.
    docker.expireComposeGrants();
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.textContaining('editing permission has expired'), findsOneWidget);
    await _unlock(tester);
    expect(docker.saves.last.apply, isFalse);
    expect(docker.composeFiles['/srv/shop/compose.yaml'], 'services: {}\n');
    expect(find.text('The running containers still use the previous configuration.'), findsOneWidget);
    await tapKey(tester, 'docker-compose-close');
  });

  testWidgets('leaving with unsaved edits asks first', (tester) async {
    await _pump(tester);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-editYaml');
    await _unlock(tester);
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), 'x: 1\n');
    await tester.pump();
    await tapKey(tester, 'docker-compose-cancel-edit');
    expect(find.text('Discard the changes?'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(_editorText(tester), 'x: 1\n');
    await tapKey(tester, 'docker-compose-cancel-edit');
    await tapKey(tester, 'confirm-ok');
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsNothing);
    expect(find.byKey(const ValueKey('docker-compose-text')), findsOneWidget);
    await tapKey(tester, 'docker-compose-close');
  });

  testWidgets('the dialog fits a phone screen', (tester) async {
    await _pump(tester, size: const Size(390, 844));
    final header = find.byKey(const ValueKey('docker-group-title-shop'));
    await tester.ensureVisible(header);
    await tester.longPress(header);
    await settle(tester);
    await tapKey(tester, 'docker-compose-menu-editYaml');
    await _unlock(tester);
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), 'services:\n\tx: 1\n');
    await tester.pump();
    await tapKey(tester, 'docker-compose-save-apply');
    expect(find.byKey(const ValueKey('docker-compose-output')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a project whose files are unknown says so', (tester) async {
    final docker = _Docker()..projectError = const DockerComposeException(DockerComposeFailure.filesUnknown);
    await _pump(tester, service: docker);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-viewYaml');
    expect(find.byKey(const ValueKey('docker-compose-load-error')), findsOneWidget);
    expect(find.textContaining('do not record which compose files'), findsOneWidget);
    expect(find.byKey(const ValueKey('docker-compose-edit')), findsNothing);
    await tapKey(tester, 'docker-compose-close');
  });

  testWidgets('compose files are masked until the passphrase; editing shows the source', (tester) async {
    await _pump(tester);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-viewYaml');
    String shown() =>
        tester.widget<SelectableText>(find.byKey(const ValueKey('docker-compose-text'))).textSpan!.toPlainText();
    expect(shown(), contains('command: ["redis-server", "--requirepass", "••••••", "--appendonly", "yes"]'));
    expect(shown(), contains('SENTRY_DSN: ••••••'));
    expect(shown(), isNot(contains('demo-redis-pass')));
    expect(find.textContaining('Passwords, tokens and keys are hidden.'), findsOneWidget);
    final lines = shown().split('\n').length;

    await tapKey(tester, 'docker-compose-reveal');
    expect(find.textContaining('show the files of shop in full'), findsOneWidget);
    await _unlock(tester, 'wrong');
    expect(find.byKey(const ValueKey('docker-compose-wrong-passphrase')), findsOneWidget);
    await _unlock(tester);
    expect(find.byKey(const ValueKey('docker-compose-editor')), findsNothing);
    expect(shown(), contains('"--requirepass", "demo-redis-pass"'));
    expect(shown().split('\n'), hasLength(lines), reason: 'masking keeps every line');
    expect(find.textContaining('Passwords, tokens and keys are hidden.'), findsNothing);

    // Hidden again; editing reuses the grant and starts from the source.
    await tapKey(tester, 'docker-compose-hide');
    expect(shown(), isNot(contains('demo-redis-pass')));
    await tapKey(tester, 'docker-compose-edit');
    expect(find.byKey(const ValueKey('docker-compose-unlock-panel')), findsNothing);
    expect(_editorText(tester), contains('demo-redis-pass'));
    await tapKey(tester, 'docker-compose-cancel-edit');
    await tapKey(tester, 'docker-compose-close');
  });

  testWidgets('a swapped file and a directory shared with other users are explained', (tester) async {
    final docker = await _pump(tester);
    await tapKey(tester, 'docker-compose-menu-button-shop');
    await tapKey(tester, 'docker-compose-menu-editYaml');
    await _unlock(tester);
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), 'services: {}\n');
    await tester.pump();
    docker.nextResult = const DockerComposeTampered('/srv/shop/compose.yaml');
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.textContaining('replaced or changed on the host by someone else'), findsOneWidget);
    docker.nextResult = const DockerComposeTampered(
      '/srv/shop/compose.yaml',
      backup: '/srv/shop/compose.yaml.bak-1',
      restored: true,
    );
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.textContaining('restored from the backup /srv/shop/compose.yaml.bak-1'), findsOneWidget);
    await tapKey(tester, 'docker-compose-cancel-edit');
    await tapKey(tester, 'confirm-ok');
    await tapKey(tester, 'docker-compose-close');

    // /opt/monitoring is writable by other users in the demo.
    await tapKey(tester, 'docker-compose-menu-button-monitoring');
    await tapKey(tester, 'docker-compose-menu-editYaml');
    await _unlock(tester);
    const edited = 'services:\n  grafana:\n    image: grafana/grafana:11.3.0\n';
    await tester.enterText(find.byKey(const ValueKey('docker-compose-editor')), edited);
    await tester.pump();
    await tapKey(tester, 'docker-compose-save-only');
    expect(find.text('Save in a shared directory?'), findsOneWidget);
    expect(find.textContaining('can be written by other users: /opt/monitoring'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(docker.composeFiles['/opt/monitoring/docker-compose.yml'], isNot(edited));
    expect(_editorText(tester), edited);
    await tapKey(tester, 'docker-compose-save-only');
    await tapKey(tester, 'confirm-ok');
    expect(docker.saves.last.allowSharedDirs, isTrue);
    expect(docker.composeFiles['/opt/monitoring/docker-compose.yml'], edited);
    await tapKey(tester, 'docker-compose-close');
  });

  test('mock reveal needs a live grant', () async {
    final docker = MockDockerService();
    final session = await docker.connect(DockerHost.localId);
    expect(await docker.composeReveal(session.id, 'shop', 'forged'), isNull);
    final grant = await docker.composeUnlock(
      session.id,
      'shop',
      PassphraseProof(SecretText(MockDockerService.demoPassphrase)),
    );
    final full = await docker.composeReveal(session.id, 'shop', grant);
    expect(full!.files.first.content!.expose(), contains('demo-redis-pass'));
    expect(full.files.first.masked, isFalse);
    final masked = await docker.composeProject(session.id, 'shop');
    expect(masked.files.first.masked, isTrue);
    expect(masked.files.first.digest, full.files.first.digest);
  });

  test('mock compose save wipes the edited text', () async {
    final docker = MockDockerService();
    final session = await docker.connect(DockerHost.localId);
    final grant = await docker.composeUnlock(
      session.id,
      'shop',
      PassphraseProof(SecretText(MockDockerService.demoPassphrase)),
    );
    final project = await docker.composeProject(session.id, 'shop');
    final content = SecretText('services: {}\n');
    final result = await docker.composeSave(session.id, 'shop', grant, [
      DockerComposeEdit(path: project.files.first.path, content: content, digest: project.files.first.digest!),
    ]);
    expect(result, isA<DockerComposeSaved>());
    expect(content.isWiped, isTrue);
    project.wipe();
    expect(project.files.first.content!.isWiped, isTrue);
  });
}
