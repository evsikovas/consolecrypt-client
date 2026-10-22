import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_notes.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Notes of a host from the inventory: the host card shows how many there
/// are, and "Notes" opens the panel on that host without connecting.
void main() {
  for (final entry in ['count', 'menu']) {
    testWidgets('the host card $entry opens the notes of that host without connecting', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'staging-web');
      await (backend.services.notes! as MockNoteService).save(
        Note(title: 'Reload nginx only', target: NoteObjectTarget(NoteObjectKind.host, host.id.value)),
      );
      await pumpApp(tester, backend);
      await enterKey(tester, 'hosts-search', 'staging-web');
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      expect(find.byKey(const ValueKey('host-notes-staging-web')), findsOneWidget);
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('host-notes-staging-web'))).label, '1');
      if (entry == 'count') {
        await tapKey(tester, 'host-notes-staging-web');
      } else {
        await tapKey(tester, 'host-menu-staging-web');
        await tapKey(tester, 'host-notes-menu-staging-web');
      }
      expect(container.read(workspaceToolsProvider).selected, WorkspaceTool.notes);
      expect(container.read(terminalTabsProvider).tabs, isEmpty, reason: 'choosing a host never connects');
      expect(container.read(routerProvider).state.uri.path, isNot(AppRoutes.terminal));
      expect(tester.widget<Text>(find.byKey(const ValueKey('notes-context'))).data, 'staging-web');
      expect(find.text('Reload nginx only'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('a host without notes shows no count', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await enterKey(tester, 'hosts-search', 'staging-web');
    expect(find.byKey(const ValueKey('host-notes-staging-web')), findsNothing);
  });
}
