import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_designer_controller.dart';
import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_designer_save_dialog.dart';
import 'package:consolecrypt/db/db_designer_view.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// The table designer inside a database workspace: designer tabs next to
/// the query tabs, the structure-tree entries and drafts, and saving
/// through the review dialog. The workspace supplies its session and the
/// editor; everything else lives here.
mixin DbDesignerWorkspace<T extends ConsumerStatefulWidget> on ConsumerState<T> {
  late final designers = DbDesignerHub(onFocus: (designer) => setState(() => activeDesigner = designer));

  /// The designer tab in front; null while a query tab is shown.
  DbDesignerController? activeDesigner;
  int _nextDesigner = 1;

  bool get designerCurrent;
  DbSessionInfo? get designerSession;
  Object? get designerBackend;
  DbConnection get designerConnection;
  DbObjectRef? get designerSelection;

  /// Appends text to the current editor tab without running it.
  void designerDraft(String text);
  void designerDiscardReview(String session, String ticket);

  DbTableDesignService? get _service {
    final backend = designerBackend;
    return backend is DbTableDesignService ? backend : null;
  }

  bool get designerAvailable {
    final session = designerSession;
    return _service != null && session != null && dbDesignerSupported(session.engine);
  }

  @override
  void initState() {
    super.initState();
    designers.addListener(_designersChanged);
  }

  @override
  void dispose() {
    designers
      ..removeListener(_designersChanged)
      ..dispose();
    super.dispose();
  }

  void _designersChanged() {
    if (mounted && designerCurrent) setState(() {});
  }

  /// Drops every designer (session closed or vault locked).
  void closeDesigners() {
    activeDesigner = null;
    designers.clear();
  }

  // Tabs -----------------------------------------------------------------------------

  String _tabTitle(DbDesignerController designer, AppLocalizations l) {
    final name = designer.title.isEmpty ? l.dbDesignerNewTab : designer.title;
    return '${l.dbDesignerTabTitle(name)}${designer.pending ? ' •' : ''}'; // l10n-ignore: unsaved marker
  }

  List<GlassTab> designerTabs(AppLocalizations l) => [
    for (final designer in designers.designers)
      GlassTab(
        key: ValueKey('db-designer-tab-${designer.id}'),
        title: _tabTitle(designer, l),
        tooltip: designer.pending ? l.dbDesignerUnsavedMark : null,
      ),
  ];

  int? designerTabIndex(int queryTabs) {
    final designer = activeDesigner;
    if (designer == null) return null;
    final index = designers.designers.indexOf(designer);
    return index < 0 ? null : queryTabs + index;
  }

  void selectDesignerTab(int index) => setState(() => activeDesigner = designers.designers[index]);

  Widget designerView(DbDesignerController designer, DbSessionInfo session, DbQueryService queryService) =>
      DbTableDesignerView(
        key: ValueKey('db-designer-view-${designer.id}'),
        designer: designer,
        session: session,
        queryService: queryService,
        onSave: () => unawaited(saveDesigner(designer)),
        onOpenInEditor: designerDraft,
      );

  // Tree -------------------------------------------------------------------------------

  /// Designer entries for a structure object: create in a schema/database
  /// (or its Tables folder), edit the structure of a table.
  List<GlassMenuEntry<String>> designerMenu(DbObjectRef object) {
    final session = designerSession;
    if (!designerAvailable || session == null) return const [];
    final l = context.l10n, engine = session.engine;
    final container = switch (object.kind) {
      DbObjectKind.schema => engine == DbEngine.postgresql,
      // MongoDB commands run in the connection's database.
      DbObjectKind.database =>
        engine != DbEngine.postgresql && (engine != DbEngine.mongodb || object.name == designerConnection.database),
      _ => false,
    };
    return [
      if (container)
        GlassMenuItem(
          key: const ValueKey('db-design-create'),
          value: 'design-create',
          label: engine == DbEngine.mongodb ? l.dbDesignerCreateCollection : l.dbDesignerCreateTable,
          icon: Icons.add_box_outlined,
        ),
      if (object.kind == DbObjectKind.table && engine != DbEngine.mongodb)
        GlassMenuItem(
          key: const ValueKey('db-design-alter'),
          value: 'design-alter',
          label: l.dbDesignerAlterTable,
          icon: Icons.design_services_outlined,
        ),
    ];
  }

  void designerAction(String action, DbObjectRef object) => switch (action) {
    'design-create' => unawaited(openDesigner(container: object)),
    'design-alter' => unawaited(openDesigner(table: object)),
    _ => null,
  };

  /// The schema or database a new table goes to without an explicit one.
  DbObjectRef _defaultContainer(DbSessionInfo session) {
    final selected = designerSelection;
    if (session.engine == DbEngine.postgresql) {
      final schema = switch (selected?.kind) {
        DbObjectKind.schema => selected!.name,
        _ => selected?.schema ?? 'public',
      };
      return DbObjectRef(database: '', kind: DbObjectKind.schema, name: schema);
    }
    final database = selected == null
        ? designerConnection.database
        : selected.kind == DbObjectKind.database
        ? selected.name
        : selected.database;
    final name = session.engine == DbEngine.mongodb || database.isEmpty ? designerConnection.database : database;
    return DbObjectRef(database: name, kind: DbObjectKind.database, name: name);
  }

  String _newName() {
    const base = 'new_table'; // l10n-ignore: default table name
    var name = base;
    for (var n = 2; designers.designers.any((d) => !d.alter && d.design.name == name); n++) {
      name = '${base}_$n';
    }
    return name;
  }

  /// Opens a designer: a new table in [container] (or the default one), or
  /// the structure of [table]. A table already open comes to the front.
  Future<void> openDesigner({DbObjectRef? container, DbObjectRef? table}) async {
    final service = _service, session = designerSession;
    if (!designerCurrent || service == null || session == null) return;
    if (table != null) {
      final open = designers.designers.where((d) => d.alter && d.isTable(table, d.original!)).firstOrNull;
      if (open != null) {
        setState(() => activeDesigner = open);
        return;
      }
    }
    try {
      final dialect = await service.designDialect(session.engine);
      if (!designerCurrent || designerSession?.id != session.id) return;
      final DbDesignerController designer;
      if (table != null) {
        final loaded = await service.loadTableDesign(session.id, table);
        if (!designerCurrent || designerSession?.id != session.id) return;
        designer = DbDesignerController.alter(
          id: _nextDesigner++,
          service: service,
          sessionId: session.id,
          dialect: dialect,
          loaded: loaded,
        );
      } else {
        designer = DbDesignerController.create(
          id: _nextDesigner++,
          service: service,
          sessionId: session.id,
          dialect: dialect,
          container: container ?? _defaultContainer(session),
          name: _newName(),
        );
      }
      setState(() {
        designers.add(designer, reveal: table == null ? container ?? _defaultContainer(session) : null);
        activeDesigner = designer;
      });
    } catch (_) {
      if (mounted && designerCurrent) showSnack(context, context.l10n.dbDesignerLoadFailed, error: true);
    }
  }

  // Close and save -------------------------------------------------------------------------

  Future<void> closeDesignerTab(int index) => closeDesigner(designers.designers[index]);

  /// Closes a designer; unsaved changes ask Save / Don't save / Cancel.
  Future<void> closeDesigner(DbDesignerController designer) async {
    if (designer.dirty) {
      final l = context.l10n;
      final choice = await showAppDialog<String>(
        context,
        secure: true,
        builder: (context) => GlassDialog(
          key: const ValueKey('db-design-unsaved'),
          title: l.dbDesignerUnsavedTitle(designer.title.isEmpty ? l.dbDesignerNewTab : designer.title),
          icon: Icons.warning_rounded,
          iconTone: GlassTone.warning,
          content: Text(l.dbDesignerUnsavedMessage),
          secondaryActions: [
            GlassButton(
              key: const ValueKey('db-design-unsaved-cancel'),
              label: l.commonCancel,
              onPressed: () => closeDialog<String>(context),
            ),
            GlassButton(
              key: const ValueKey('db-design-unsaved-discard'),
              label: l.dbDesignerDontSave,
              onPressed: () => closeDialog(context, 'discard'),
            ),
          ],
          primaryAction: GlassButton.prominent(
            key: const ValueKey('db-design-unsaved-save'),
            label: l.dbDesignerSaveButton,
            onPressed: () => closeDialog(context, 'save'),
          ),
        ),
      );
      if (!mounted || !designers.designers.contains(designer) || choice == null) return;
      if (choice == 'save' && !await saveDesigner(designer)) return;
    }
    if (!mounted || !designers.designers.contains(designer)) return;
    setState(() {
      if (identical(activeDesigner, designer)) activeDesigner = null;
      designers.remove(designer);
    });
  }

  DbObjectRef _containerOf(DbDesignerController designer) {
    final d = designer.design;
    return designer.engine == DbEngine.postgresql
        ? DbObjectRef(database: '', kind: DbObjectKind.schema, name: d.schema ?? 'public')
        : DbObjectRef(database: d.database, kind: DbObjectKind.database, name: d.database);
  }

  /// The save dialog (preview + the one confirmation). True when the
  /// script was applied.
  Future<bool> saveDesigner(DbDesignerController designer) async {
    final service = _service, session = designerSession, backend = designerBackend;
    if (!designerCurrent || service == null || session == null || backend is! DbQueryService) return false;
    await designer.refreshPlan();
    if (!mounted || !designerCurrent || !designer.canSave) return false;
    final plan = designer.plan, l = context.l10n;
    final outcome = await showAppDialog<DbDesignSaveOutcome>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => DbDesignSaveDialog(
        title: designer.title,
        plan: plan,
        session: designerSession ?? session,
        review: () => backend.reviewQuery(session.id, DbQueryRequest(text: plan.script)),
        apply: (ticket) => service.applyDesign(session.id, DbQueryRequest(text: plan.script, approvalTicket: ticket)),
        discard: (ticket) async => designerDiscardReview(session.id, ticket),
        onCopy: () => unawaited(copyPlainWithNotice(context, ref, plan.script, what: l.copyWhatDdl)),
      ),
    );
    if (!mounted || !designerCurrent || designerSession?.id != session.id) return false;
    switch (outcome) {
      case DbDesignSaveOutcome.openInEditor:
        designerDraft(plan.script);
        return false;
      case DbDesignSaveOutcome.applied:
        final before = designer.alter ? designer.tableRef(designer.original) : null;
        DbTableDesign? reloaded;
        if (designer.dialect.has('alter')) {
          try {
            reloaded = await service.loadTableDesign(session.id, designer.tableRef());
          } catch (_) {}
        }
        if (!mounted || !designers.designers.contains(designer)) return true;
        designer.saved(reloaded);
        designers.reload([_containerOf(designer), ?before, designer.tableRef()]);
        showSnack(context, l.dbDesignerSaved(designer.title));
        return true;
      case null:
        return false;
    }
  }
}
