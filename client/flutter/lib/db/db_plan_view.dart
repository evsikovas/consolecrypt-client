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
  final node = value['Plan'] is Map ? value['Plan'] as Map : value; // l10n-ignore: plan key
  final type = node['Node Type']; // l10n-ignore: plan key
  if (type is! String) return null;
  final target = [
    for (final key in ['Relation Name', 'Index Name', 'Function Name', 'CTE Name']) // l10n-ignore: plan key
      if (node[key] is String) node[key] as String,
  ];
  final details = [
    for (final key in [
      'Join Type', // l10n-ignore: plan key
      'Index Cond', // l10n-ignore: plan key
      'Hash Cond', // l10n-ignore: plan key
      'Merge Cond', // l10n-ignore: plan key
      'Filter', // l10n-ignore: plan key
      'Sort Key', // l10n-ignore: plan key
      'Group Key', // l10n-ignore: plan key
      'Description', // l10n-ignore: plan key
      'Indexes', // l10n-ignore: plan key
    ])
      if (node[key] != null)
        '$key: ${node[key] is List ? (node[key] as List).join(', ') : node[key]}', // l10n-ignore: plan field
  ];
  final loops = _number(node['Actual Loops']) ?? 1; // l10n-ignore: plan key
  final actual = _number(node['Actual Total Time']); // l10n-ignore: plan key
  return DbPlanNode(
    label: target.isEmpty ? type : '$type · ${target.join(' · ')}', // l10n-ignore: plan label
    detail: details.join('\n'),
    rows: _number(node['Actual Rows']) ?? _number(node['Plan Rows']), // l10n-ignore: plan key
    timeMs: actual == null ? null : actual * loops,
    cost: _number(node['Total Cost']), // l10n-ignore: plan key
    children: [for (final child in (node['Plans'] as List? ?? const [])) ?_pgNode(child)], // l10n-ignore: plan key
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

/// SQLite `EXPLAIN QUERY PLAN` steps: `{"detail", "children"}`.
DbPlanNode _sqliteNode(Object? value) {
  final step = value is Map ? value : const <String, Object?>{};
  return DbPlanNode(
    label: '${step['detail'] ?? ''}', // l10n-ignore: plan text
    children: [for (final child in (step['children'] as List? ?? const [])) _sqliteNode(child)],
  );
}

/// Cassandra / ScyllaDB query trace (`cql_trace`): the coordinator's session,
/// then each node with its events. An event's time is the gap since the
/// previous event of its node, so the hottest bar is the slowest step.
DbPlanNode _traceNode(Map<Object?, Object?> trace) {
  double? ms(Object? micros) => _number(micros) == null ? null : _number(micros)! / 1000;
  final nodes = [
    for (final node in (trace['nodes'] as List? ?? const []).whereType<Map<Object?, Object?>>())
      () {
        var previous = 0.0;
        final events = [
          for (final event in (node['events'] as List? ?? const []).whereType<Map<Object?, Object?>>())
            () {
              final at = ms(event['elapsed_us']) ?? previous;
              final step = DbPlanNode(
                label: '${event['activity'] ?? ''}', // l10n-ignore: trace text
                detail: '${event['thread'] ?? ''}', // l10n-ignore: trace text
                timeMs: (at - previous).clamp(0, double.infinity).toDouble(),
              );
              previous = at;
              return step;
            }(),
        ];
        return DbPlanNode(label: '${node['source'] ?? ''}', timeMs: previous, children: events); // l10n-ignore: address
      }(),
  ];
  return DbPlanNode(
    label: ['${trace['request'] ?? 'TRACE'}', ?trace['coordinator'] as String?].join(' · '), // l10n-ignore: trace text
    detail: [for (final MapEntry(:key, :value) in ((trace['parameters'] as Map?) ?? const {}).entries) '$key: $value']
        .join('\n'),
    timeMs: ms(trace['duration_us']),
    children: nodes,
  );
}

/// The plan tree of a native EXPLAIN document, or null for text/unknown plans.
DbPlanNode? parsePlan(DbJson document) {
  final plan = document['plan'] ?? document;
  const trace = 'cql_trace'; // l10n-ignore: plan key
  if (plan is Map && plan[trace] is Map) return _traceNode(plan[trace] as Map<Object?, Object?>);
  const sqlite = 'sqlite_plan'; // l10n-ignore: plan key
  if (plan is Map && plan[sqlite] is List) {
    return DbPlanNode(
      label: 'QUERY PLAN', // l10n-ignore: SQL keyword
      children: [for (final step in plan[sqlite] as List) _sqliteNode(step)],
    );
  }
  if (plan is List && plan.isNotEmpty) return _pgNode(plan.first);
  if (plan is Map && plan['Plan'] != null) return _pgNode(plan); // l10n-ignore: plan key
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
