/// What a SQL generator (the table designer, DDL helpers) needs to know about
/// SQLite in one place: declared types, identifier quoting, literals and what
/// `ALTER TABLE` can change without rebuilding the table.
abstract final class DbSqliteDialect {
  /// Storage classes, in the order a type list shows them. STRICT tables
  /// accept exactly these (plus ANY).
  static const storageClasses = ['INTEGER', 'TEXT', 'REAL', 'BLOB', 'NUMERIC'];

  /// Declared types people use; SQLite maps each to an affinity by name.
  static const declaredTypes = [
    'INTEGER',
    'TEXT',
    'REAL',
    'BLOB',
    'NUMERIC',
    'BOOLEAN',
    'DATE',
    'DATETIME',
    'VARCHAR(255)',
    'DECIMAL(10,2)',
    'DOUBLE',
    'BIGINT',
  ];

  /// Types a STRICT table allows.
  static const strictTypes = ['INT', 'INTEGER', 'REAL', 'TEXT', 'BLOB', 'ANY'];

  /// Table options after the column list.
  static const tableOptions = ['STRICT', 'WITHOUT ROWID']; // l10n-ignore: SQL keywords

  /// `ALTER TABLE` forms; any other change needs a new table, a copy of the
  /// rows and a rename (in one transaction, foreign keys off).
  static const alterForms = ['RENAME TO', 'RENAME COLUMN', 'ADD COLUMN', 'DROP COLUMN']; // l10n-ignore: SQL keywords

  /// The affinity SQLite gives a declared type (datatype3 rules).
  static String affinity(String declared) {
    final t = declared.toUpperCase();
    if (t.contains('INT')) return 'INTEGER';
    if (t.contains('CHAR') || t.contains('CLOB') || t.contains('TEXT')) return 'TEXT';
    if (t.isEmpty || t.contains('BLOB')) return 'BLOB';
    if (t.contains('REAL') || t.contains('FLOA') || t.contains('DOUB')) return 'REAL';
    return 'NUMERIC';
  }

  /// `"name"` with embedded quotes doubled.
  static String quoteIdentifier(String name) => '"${name.replaceAll('"', '""')}"';

  /// An SQL literal: NULL, numbers as written, booleans as 1/0, text in
  /// single quotes with quotes doubled (no backslash escapes), bytes as X'…'.
  static String literal(Object? value) => switch (value) {
    null => 'NULL',
    final bool b => b ? '1' : '0',
    final num n when n.isFinite => '$n',
    final List<int> bytes => "X'${bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join().toUpperCase()}'",
    _ => "'${'$value'.replaceAll("'", "''")}'",
  };
}
