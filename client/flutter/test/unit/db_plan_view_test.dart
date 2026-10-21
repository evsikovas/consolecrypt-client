import 'package:consolecrypt/db/db_plan_view.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('PostgreSQL and ClickHouse plans become a tree with time, rows and cost', () {
    final root = parsePlan({
      'plan': [
        {
          'Plan': {
            'Node Type': 'Hash Join',
            'Join Type': 'Inner',
            'Total Cost': 120.5,
            'Actual Total Time': 4.2,
            'Actual Loops': 1,
            'Actual Rows': 10,
            'Plans': [
              {'Node Type': 'Seq Scan', 'Relation Name': 'orders', 'Plan Rows': 1000, 'Total Cost': 80},
              {'Node Type': 'Index Scan', 'Index Name': 'customers_pkey', 'Actual Total Time': 0.5, 'Actual Loops': 3},
            ],
          },
        },
      ],
    })!;
    expect(root.label, 'Hash Join');
    expect(root.detail, contains('Join Type: Inner'));
    expect(root.timeMs, 4.2);
    expect(root.children.map((c) => c.label), ['Seq Scan · orders', 'Index Scan · customers_pkey']);
    expect(root.children[0].rows, 1000);
    expect(root.children[1].timeMs, 1.5);
  });

  test('MySQL JSON plans list their tables and costs', () {
    final root = parsePlan({
      'plan': {
        'query_block': {
          'cost_info': {'query_cost': '2.25'},
          'table': {'table_name': 'cc_items', 'access_type': 'ALL', 'rows_examined_per_scan': 2, 'key': 'PRIMARY'},
        },
      },
    })!;
    expect(root.cost, 2.25);
    expect(root.children.single.label, 'table · cc_items · ALL');
    expect(root.children.single.rows, 2);
  });

  test('text plans are not parsed as trees', () {
    expect(parsePlan({'plan': '-> Table scan on t'}), isNull);
  });
}
