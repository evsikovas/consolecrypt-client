import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/db/db_document_view.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:material_ui/material_ui.dart';

/// One step of a query plan, normalized across engines.
final class DbPlanNode {
  DbPlanNode({required this.label, this.detail = '', this.rows, this.timeMs, this.cost, this.children = const []});
  final String label, detail;
  final double? rows, timeMs, cost;
  final List<DbPlanNode> children;
}

double? _number(Object? value) => switch (value) {
  final num n => n.toDouble(),
  final String s => double.tryParse(s),
  _ => null,
};

/// PostgreSQL and ClickHouse JSON plans: `{"Plan": {"Node Type", "Plans": [...]}}`.
DbPlanNode? _pgNode(Object? value) {
  if (value is! Map) return null;
  final node = value['Plan'] is Map ? value['Plan'] as Map : value;
  final type = node['Node Type'];
  if (type is! String) return null;
  final target = [
    for (final key in ['Relation Name', 'Index Name', 'Function Name', 'CTE Name'])
      if (node[key] is String) node[key] as String,
  ];
  final details = [
    for (final key in [
      'Join Type',
      'Index Cond',
      'Hash Cond',
      'Merge Cond',
      'Filter',
      'Sort Key',
      'Group Key',
      'Description',
      'Indexes',
    ])
      if (node[key] != null)
        '$key: ${node[key] is List ? (node[key] as List).join(', ') : node[key]}', // l10n-ignore: plan field
  ];
  final loops = _number(node['Actual Loops']) ?? 1;
  final actual = _number(node['Actual Total Time']);
  return DbPlanNode(
    label: target.isEmpty ? type : '$type · ${target.join(' · ')}', // l10n-ignore: plan label
    detail: details.join('\n'),
    rows: _number(node['Actual Rows']) ?? _number(node['Plan Rows']),
    timeMs: actual == null ? null : actual * loops,
    cost: _number(node['Total Cost']),
    children: [for (final child in (node['Plans'] as List? ?? const [])) ?_pgNode(child)],
  );
}

/// MySQL `EXPLAIN FORMAT=JSON`: nested operations around `table` entries.
DbPlanNode _mysqlNode(String name, Object? value) {
  if (value is List) {
    return DbPlanNode(label: name, children: [for (final item in value) _mysqlNode(name, item)]);
  }
  if (value is! Map) return DbPlanNode(label: name, detail: '$value');
  final table = value['table_name'] as String?;
  final access = value['access_type'] as String?;
  final cost = value['cost_info'] is Map ? value['cost_info'] as Map : const <String, Object?>{};
  final children = <DbPlanNode>[];
  for (final entry in value.entries) {
    final child = entry.value;
    if (child is Map && entry.key != 'cost_info' || child is List && child.any((e) => e is Map)) {
      children.add(_mysqlNode(entry.key as String, child));
    }
  }
  return DbPlanNode(
    label: [name, ?table, ?access].join(' · '),
    detail: [
      if (value['key'] != null) 'key: ${value['key']}', // l10n-ignore: plan field
      if (value['attached_condition'] != null) 'condition: ${value['attached_condition']}', // l10n-ignore: plan field
      if (value['filtered'] != null) 'filtered: ${value['filtered']}%', // l10n-ignore: plan field
    ].join('\n'),
    rows: _number(value['rows_examined_per_scan']) ?? _number(value['rows_produced_per_join']),
    cost: _number(cost['query_cost']) ?? _number(cost['prefix_cost']),
    children: children,
  );
}

/// The plan tree of a native EXPLAIN document, or null for text/unknown plans.
DbPlanNode? parsePlan(DbJson document) {
  final plan = document['plan'] ?? document;
  if (plan is List && plan.isNotEmpty) return _pgNode(plan.first);
  if (plan is Map && plan['Plan'] != null) return _pgNode(plan);
  const block = 'query_block'; // l10n-ignore: plan key
  if (plan is Map && plan[block] != null) return _mysqlNode(block, plan[block]);
  return null;
}

/// Indented plan steps with a bar for each step's share of time (or cost).
class DbPlanView extends StatefulWidget {
  const DbPlanView({required this.document, super.key});
  final DbJson document;
  @override
  State<DbPlanView> createState() => _DbPlanViewState();
}

class _DbPlanViewState extends State<DbPlanView> {
  bool _raw = false;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, theme = Theme.of(context);
    final root = parsePlan(widget.document);
    final text = widget.document['plan'];
    if (_raw || root == null) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (root != null) Align(alignment: Alignment.centerRight, child: _toggle(l)),
          Expanded(
            child: text is String
                ? SingleChildScrollView(
                    child: SelectableText(text, style: const TextStyle(fontFamily: 'monospace')),
                  )
                : DbDocumentView(document: widget.document),
          ),
        ],
      );
    }
    final rows = <(DbPlanNode, int)>[];
    void walk(DbPlanNode node, int depth) {
      if (rows.length >= 2000) return;
      rows.add((node, depth));
      for (final child in node.children) {
        walk(child, depth + 1);
      }
    }

    walk(root, 0);
    final byTime = rows.any((r) => r.$1.timeMs != null);
    double weight(DbPlanNode node) => (byTime ? node.timeMs : node.cost) ?? 0;
    // Self weight: what a step adds beyond its children.
    double self(DbPlanNode node) =>
        (weight(node) - node.children.fold<double>(0, (sum, child) => sum + weight(child))).clamp(0, double.infinity);
    final total = weight(root) > 0 ? weight(root) : 1.0;
    final hottest = rows.map((r) => self(r.$1)).fold<double>(0, (a, b) => a > b ? a : b);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(child: Text(byTime ? l.dbPlanByTime : l.dbPlanByCost, style: theme.textTheme.bodySmall)),
            _toggle(l),
          ],
        ),
        Expanded(
          child: ListView.builder(
            itemCount: rows.length,
            itemBuilder: (context, index) {
              final (node, depth) = rows[index];
              final share = self(node) / total;
              final hot = hottest > 0 && self(node) == hottest;
              final facts = [
                if (node.timeMs != null) l.dbPlanTime(node.timeMs!.toStringAsFixed(node.timeMs! < 10 ? 2 : 0)),
                if (node.rows != null) l.dbPlanRows(node.rows!.round()),
                if (node.cost != null) l.dbPlanCost(node.cost!.toStringAsFixed(1)),
              ];
              return Padding(
                padding: EdgeInsets.only(left: 16.0 * depth.clamp(0, 12), top: 4, bottom: 4, right: 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        if (hot) Icon(Icons.local_fire_department, size: 16, color: theme.colorScheme.error),
                        Expanded(
                          child: Text(
                            node.label,
                            style: theme.textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
                          ),
                        ),
                        Text(facts.join(' · '), style: theme.textTheme.bodySmall),
                      ],
                    ),
                    const SizedBox(height: 3),
                    ClipRRect(
                      borderRadius: BorderRadius.circular(2),
                      child: LinearProgressIndicator(
                        value: share.clamp(0, 1),
                        minHeight: 4,
                        color: hot ? theme.colorScheme.error : theme.colorScheme.primary,
                        backgroundColor: theme.colorScheme.surfaceContainerHighest,
                      ),
                    ),
                    if (node.detail.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(top: 2),
                        child: SelectableText(
                          node.detail,
                          style: theme.textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
                        ),
                      ),
                  ],
                ),
              );
            },
          ),
        ),
      ],
    );
  }

  Widget _toggle(AppLocalizations l) =>
      TextButton(onPressed: () => setState(() => _raw = !_raw), child: Text(_raw ? l.dbPlanTree : l.dbPlanJson));
}
