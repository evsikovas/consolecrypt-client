import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const kind = KubeResourceKind(
  group: '',
  version: 'v1',
  resource: 'pods',
  kind: 'Pod',
  namespaced: true,
  verbs: ['get'],
);
const columns = [
  KubeTableColumn(name: 'Name', type: 'string', format: 'name', description: '', priority: 0),
  KubeTableColumn(name: 'Restarts', type: 'integer', format: '', description: '', priority: 0),
];

void main() {
  Future<void> pump(
    WidgetTester tester,
    List<KubeResourceRow> rows, {
    Size size = const Size(1200, 700),
    double scale = 1,
    Locale locale = const Locale('en'),
    ValueChanged<KubeResourceRow>? onOpen,
  }) async {
    tester.view.physicalSize = size;
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp(
          theme: AppTheme.light(),
          locale: locale,
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
            child: child!,
          ),
          home: Scaffold(
            body: KubeResourceTableView(
              rows: rows,
              kind: kind,
              columns: columns,
              selected: const {},
              canSelect: true,
              onSelection: (_) {},
              onOpen: onOpen ?? (_) {},
              hasMore: false,
              loadingMore: false,
              onMore: () {},
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('10,000 rows stay virtualized and sort/open by stable resource identity', (tester) async {
    final rows = List.generate(
      10000,
      (i) => KubeResourceRow(
        name: 'pod-${i.toString().padLeft(5, '0')}',
        uid: 'uid-$i',
        resourceVersion: '1',
        namespace: 'default',
        status: 'Running',
        cells: ['pod-$i', '$i'],
      ),
    );
    KubeResourceRow? opened;
    await pump(tester, rows, onOpen: (row) => opened = row);
    final built = find.byWidgetPredicate(
      (widget) =>
          widget.key is ValueKey<String> &&
          (widget.key! as ValueKey<String>).value.startsWith('kube-resource-') &&
          widget.key != const ValueKey('kube-resource-list'),
    );
    expect(built.evaluate().length, lessThan(80));
    expect(find.byKey(const ValueKey('kube-resource-uid-9999')), findsNothing);
    await tester.tap(find.widgetWithText(TextButton, 'Name'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('kube-resource-uid-9999')));
    expect(opened?.uid, 'uid-9999');
    expect(built.evaluate().length, lessThan(80));
    expect(tester.takeException(), isNull);
  });

  testWidgets('Russian phone at 200% text stays readable without horizontal overflow', (tester) async {
    await pump(
      tester,
      const [
        KubeResourceRow(
          name: 'long-deployment-replica-pod-name-that-wraps',
          uid: 'mobile',
          resourceVersion: '1',
          namespace: 'очень-длинное-пространство-имён',
          status: 'CrashLoopBackOff',
        ),
      ],
      size: const Size(390, 844),
      scale: 2,
      locale: const Locale('ru'),
    );
    expect(find.byKey(const ValueKey('kube-resource-mobile')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('fresh watch summary replaces stale table cells', (tester) async {
    await pump(tester, const [
      KubeResourceRow(name: 'pod', uid: 'one', resourceVersion: '1', status: 'Running', cells: ['pod', '42']),
    ]);
    await pump(tester, const [KubeResourceRow(name: 'pod', uid: 'one', resourceVersion: '2', status: 'Pending')]);
    expect(find.text('Running'), findsNothing);
    expect(find.text('Pending'), findsOneWidget);
    expect(find.text('42'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('keyboard navigation crosses virtualized rows and opens the focused identity', (tester) async {
    final rows = List.generate(
      100,
      (i) => KubeResourceRow(name: 'pod-${i.toString().padLeft(3, '0')}', uid: 'uid-$i', resourceVersion: '1'),
    );
    KubeResourceRow? opened;
    await pump(tester, rows, onOpen: (row) => opened = row);
    tester.widget<Focus>(find.byKey(const ValueKey('kube-table-keyboard'))).focusNode!.requestFocus();
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.end);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('kube-resource-uid-99')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    expect(opened?.uid, 'uid-98');
    await tester.sendKeyEvent(LogicalKeyboardKey.home);
    await tester.pumpAndSettle();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    expect(opened?.uid, 'uid-0');
    expect(tester.takeException(), isNull);
  });

  testWidgets('phone sorting orders numeric native columns and reverses direction', (tester) async {
    const rows = [
      KubeResourceRow(name: 'a', uid: 'one', resourceVersion: '1', cells: ['a', '20']),
      KubeResourceRow(name: 'b', uid: 'two', resourceVersion: '1', cells: ['b', '3']),
    ];
    await pump(tester, rows, size: const Size(390, 844));
    await tester.tap(find.byKey(const ValueKey('kube-mobile-sort')));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byWidgetPredicate((widget) => widget is CheckedPopupMenuItem<String> && widget.value == 'native:1:Restarts'),
    );
    await tester.pumpAndSettle();
    expect(
      tester.getTopLeft(find.byKey(const ValueKey('kube-resource-two'))).dy,
      lessThan(tester.getTopLeft(find.byKey(const ValueKey('kube-resource-one'))).dy),
    );
    await tester.tap(find.byKey(const ValueKey('kube-mobile-sort')));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byWidgetPredicate((widget) => widget is CheckedPopupMenuItem<String> && widget.value == 'native:1:Restarts'),
    );
    await tester.pumpAndSettle();
    expect(
      tester.getTopLeft(find.byKey(const ValueKey('kube-resource-one'))).dy,
      lessThan(tester.getTopLeft(find.byKey(const ValueKey('kube-resource-two'))).dy),
    );
    expect(tester.takeException(), isNull);
  });
}
