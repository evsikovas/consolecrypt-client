import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/automation/automation_wsl_prerequisite_wizard.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../unit/automation_windows_prerequisites_test.dart'
    show windowsFact, originalScope;

const distro = LocalShellProfile(
  id: 'wsl:Ubuntu',
  kind: 'wsl',
  label: 'Ubuntu',
  shell: 'bash',
  distribution: 'Ubuntu',
);
const ps = LocalShellProfile(
  id: 'windows:powershell',
  kind: 'powershell',
  label: 'PowerShell',
  shell: 'powershell',
);
const initial = LocalShellProfiles(
  profiles: [ps, distro],
  wslDiscovery: WslDiscoveryStatus.available,
);
const paths = {
  'python': '/home/runner/venv/bin/python',
  'module_dir': '/home/runner/src',
  'root': '/home/runner/state',
};

class Port
    implements
        AutomationWindowsPrerequisiteService,
        AutomationRunnerSetupService {
  final requests = <AutomationJson>[], cancelled = <String>[];
  Completer<AutomationJson>? pending;
  int windows = 0;
  @override
  Future<AutomationJson> inspectWindowsPrerequisites({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
  }) {
    windows++;
    expect(profileId, originalScope.profileId);
    expect(notificationScopeId, originalScope.nonce);
    return pending?.future ?? Future.value(windowsFact());
  }

  @override
  Future<AutomationJson> setupRunner(
    AutomationJson target, {
    required String operationId,
    bool install = false,
  }) async {
    requests.add({...target, 'install': install});
    return {
      'version': 1,
      'users': ['runner'],
      'user': 'runner',
      'non_root': true,
      'wsl2': true,
      'missing': <String>[],
      'candidates': [paths],
      'outdated_candidates': <String>[],
      'python': '3.13',
      'system': 'Linux',
      'machine': 'x86_64',
      'release': '6.6',
    };
  }

  @override
  Future<void> cancelRunnerSetup(String operationId) async {
    cancelled.add(operationId);
  }
}

Future<void> mount(
  WidgetTester tester,
  Widget wizard, {
  Locale locale = const Locale('en'),
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [
        AppLocalizations.delegate,
        ...GlobalMaterialLocalizations.delegates,
      ],
      home: Scaffold(body: SingleChildScrollView(child: wizard)),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> click(WidgetTester tester, String key) async {
  final f = find.byKey(ValueKey(key));
  await tester.ensureVisible(f);
  await tester.pumpAndSettle();
  await tester.tap(f);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'opening does no work; reviewed terminal selection survives a real discovery refresh without command dispatch',
    (tester) async {
      final port = Port(), opened = <LocalShellProfile>[];
      var discoveries = 0;
      await mount(
        tester,
        AutomationWslPrerequisiteWizard(
          original: () => true,
          windows: port,
          setup: port,
          nativeScope: originalScope,
          initialProfiles: initial,
          discover: () async {
            discoveries++;
            return const LocalShellProfiles(
              profiles: [
                LocalShellProfile(
                  id: 'windows:powershell',
                  kind: 'powershell',
                  label: 'PowerShell refreshed',
                  shell: 'powershell',
                ),
                distro,
              ],
              wslDiscovery: WslDiscoveryStatus.available,
            );
          },
          openTerminal: (p) async {
            opened.add(p);
          },
        ),
      );
      expect(port.windows, 0);
      expect(port.requests, isEmpty);
      expect(opened, isEmpty);
      expect(discoveries, 0);
      await click(tester, 'wsl-wizard-review-plan');
      tester
          .widget<GlassSelect<String?>>(
            find.byKey(const ValueKey('wsl-wizard-terminal')),
          )
          .onChanged!('windows:powershell');
      await tester.pumpAndSettle();
      await click(tester, 'wsl-wizard-discover');
      await click(tester, 'wsl-wizard-open-terminal');
      expect(discoveries, 1);
      expect(opened.single.label, 'PowerShell refreshed');
      expect(port.requests, isEmpty);
      expect(port.windows, 0);
    },
  );
  testWidgets(
    'explicit Windows read shows unknown elevation-required and never presents a disabled or ready assertion',
    (tester) async {
      final port = Port();
      await mount(
        tester,
        AutomationWslPrerequisiteWizard(
          original: () => true,
          windows: port,
          setup: port,
          nativeScope: originalScope,
          discover: () async => initial,
          openTerminal: (_) async {},
        ),
      );
      await click(tester, 'wsl-wizard-inspect-windows');
      expect(port.windows, 1);
      expect(
        find.textContaining('reading this feature requires'),
        findsNWidgets(2),
      );
      expect(find.textContaining('2026-11-13T10:20:30.123Z'), findsOneWidget);
      expect(find.text('Disabled'), findsNothing);
      expect(port.requests, isEmpty);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets(
    'existing prepared environment is read-only then explicitly fills the form; editing the user invalidates that selection',
    (tester) async {
      final port = Port(), selected = <WslPreparedSelection>[];
      await mount(
        tester,
        AutomationWslPrerequisiteWizard(
          original: () => true,
          setup: port,
          nativeScope: originalScope,
          initialProfiles: initial,
          initialDistribution: 'Ubuntu',
          initialUser: 'runner',
          discover: () async => initial,
          openTerminal: (_) async {},
          onPrepared: selected.add,
        ),
      );
      await click(tester, 'wsl-wizard-inspect-linux');
      expect(port.requests, [
        {
          'kind': 'wsl',
          'distribution': 'Ubuntu',
          'user': 'runner',
          'install': false,
        },
      ]);
      expect(selected, isEmpty);
      tester
          .widget<GlassSelect<int?>>(
            find.byKey(const ValueKey('wsl-wizard-candidate')),
          )
          .onChanged!(0);
      await tester.pumpAndSettle();
      await click(tester, 'wsl-wizard-use-prepared');
      expect(selected.single.paths, paths);
      expect(selected.single.distribution, 'Ubuntu');
      await tester.enterText(
        find.byKey(const ValueKey('wsl-wizard-user')),
        'other',
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('wsl-wizard-use-prepared')),
        findsNothing,
      );
      expect(selected, hasLength(1));
    },
  );
  testWidgets(
    'original owner retirement cancels captured pending operation and late completion publishes no facts',
    (tester) async {
      final port = Port()..pending = Completer<AutomationJson>();
      var original = true;
      await mount(
        tester,
        AutomationWslPrerequisiteWizard(
          original: () => original,
          windows: port,
          setup: port,
          nativeScope: originalScope,
          discover: () async => initial,
          openTerminal: (_) async {},
        ),
      );
      final f = find.byKey(const ValueKey('wsl-wizard-inspect-windows'));
      await tester.ensureVisible(f);
      await tester.tap(f);
      await tester.pump();
      expect(port.windows, 1);
      original = false;
      await tester.pumpWidget(const SizedBox.shrink());
      expect(port.cancelled, hasLength(1));
      port.pending!.complete(windowsFact());
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets(
    'Russian small-window unavailable state keeps manual plan accessible without capability or mutation',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(420, 650));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await mount(
        tester,
        AutomationWslPrerequisiteWizard(
          original: () => true,
          discover: () async => const LocalShellProfiles(
            wslDiscovery: WslDiscoveryStatus.executableMissing,
          ),
          openTerminal: (_) async {},
        ),
        locale: const Locale('ru'),
      );
      expect(
        tester
            .widget<OutlinedButton>(
              find.byKey(const ValueKey('wsl-wizard-inspect-windows')),
            )
            .onPressed,
        isNull,
      );
      await click(tester, 'wsl-wizard-review-plan');
      await click(tester, 'wsl-wizard-discover');
      expect(find.textContaining('не означает выключено'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
