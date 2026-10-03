import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

Host host(String id, String name, String address, {String? username, String notes = ''}) => Host(
  id: ObjectId(id),
  name: name,
  address: address,
  username: username,
  notes: notes,
  createdAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
);

void main() {
  test('RDP default endpoint is searchable without SSH inheritance', () {
    final rdp = Host(
      id: ObjectId.generate(),
      name: 'Desktop',
      address: 'windows.example.test',
      protocol: HostProtocol.rdp,
      username: 'operator',
      createdAt: DateTime.utc(2026),
      updatedAt: DateTime.utc(2026),
    );
    expect(searchPaletteHosts([rdp], query: 'windows.example.test:3389'), [rdp]);
    expect(searchPaletteHosts([rdp], query: 'windows.example.test:22'), isEmpty);
  });

  test('exact IP/name precedes partial name and username matches', () {
    final user = host('user', 'A server', '192.0.2.1', username: 'db');
    final partial = host('partial', 'Database db cluster', '192.0.2.2');
    final prefix = host('prefix', 'DB replica', '192.0.2.3');
    final exact = host('exact', 'DB', '192.0.2.4');
    final source = [user, partial, prefix, exact];
    expect(searchPaletteHosts(source, query: 'db'), [exact, prefix, partial, user]);
    expect(searchPaletteHosts(source, query: '192.0.2.4'), [exact]);
    expect(source, [user, partial, prefix, exact], reason: 'sorting never mutates the inventory');
  });

  test('case-insensitive terms can span name, address and username', () {
    final match = host('match', 'Production database', 'db.example.org', username: 'operator');
    final other = host('other', 'Development database', 'db-dev.example.org', username: 'builder');
    expect(searchPaletteHosts([other, match], query: '  PRODUCTION   DB.EXAMPLE   OPERATOR  '), [match]);
    expect(searchPaletteHosts([match], query: 'production missing'), isEmpty);
    expect(searchPaletteHosts([match], query: 'operator@db.example.org'), [match]);
  });

  test('resolved usernames/ports are searchable without reading credential contents', () {
    final inherited = host('inherit', 'Replica', 'replica.example.org');
    final endpoints = {inherited.id: (username: 'group-operator', port: 2200)};
    expect(searchPaletteHosts([inherited], query: 'GROUP-OPERATOR', endpoints: endpoints), [inherited]);
    expect(searchPaletteHosts([inherited], query: 'replica.example.org:2200', endpoints: endpoints), [inherited]);
    expect(searchPaletteHosts([inherited], query: 'group-operator'), isEmpty);
  });

  test('IPv6 results accept an unambiguous bracketed connection label', () {
    final ipv6 = host('ipv6', 'IPv6 demo', '2001:db8::10', username: 'operator');
    final endpoints = {ipv6.id: (username: 'operator', port: 2222)};
    expect(searchPaletteHosts([ipv6], query: '2001:DB8::10', endpoints: endpoints), [ipv6]);
    expect(searchPaletteHosts([ipv6], query: 'operator@[2001:db8::10]:2222', endpoints: endpoints), [ipv6]);
  });

  test('private notes are outside the metadata search', () {
    final item = host('notes', 'Demo', '192.0.2.5', notes: 'not-indexed-note');
    expect(searchPaletteHosts([item], query: 'not-indexed-note'), isEmpty);
  });

  test('empty query gives a bounded deterministic list and invalid limits give no results', () {
    final items = [
      for (var i = 10; i >= 0; i--) host('id-$i', 'Server ${i.toString().padLeft(2, '0')}', '192.0.2.${i + 1}'),
    ];
    final result = searchPaletteHosts(items, query: ' ');
    expect(result, hasLength(8));
    expect(result.first.name, 'Server 00');
    expect(result.last.name, 'Server 07');
    expect(searchPaletteHosts(items, query: '', limit: 2), hasLength(2));
    expect(searchPaletteHosts(items, query: '', limit: 0), isEmpty);
    expect(searchPaletteHosts(items, query: '', limit: -1), isEmpty);
  });

  test('local snippets match name, description, template and tags together', () {
    final snippet = Snippet(
      id: ObjectId.generate(),
      name: 'Disk usage',
      description: 'Free space per filesystem',
      template: 'df -h',
      tags: const ['disk', 'monitoring'],
      snippetType: SnippetType.bash,
      createdAt: DateTime.utc(2026),
      updatedAt: DateTime.utc(2026),
    );
    for (final query in ['DISK SPACE', 'df -h', 'monitoring filesystem']) {
      expect(searchPaletteSnippets([snippet], query: query).single.snippet, snippet);
    }
    expect(searchPaletteSnippets([snippet], query: 'disk absent'), isEmpty);
    expect(searchPaletteSnippets([snippet], query: 'df -h').single.matchKind, SearchMatchKind.exact);
  });

  test('exact snippet result precedes a popular partial match; empty query keeps frequent entries', () {
    final now = DateTime.utc(2026);
    Snippet snippet(String name, int usage) => Snippet(
      id: ObjectId.generate(),
      name: name,
      template: 'echo demo',
      snippetType: SnippetType.bash,
      usageCount: usage,
      createdAt: now,
      updatedAt: now,
    );
    final popular = snippet('Disk overview', 50);
    final exact = snippet('Disk', 0);
    expect(searchPaletteSnippets([popular, exact], query: 'disk').map((hit) => hit.snippet), [exact, popular]);
    expect(searchPaletteSnippets([popular, exact], query: '', limit: 1).single.snippet, popular);
    expect(searchPaletteSnippets([popular, exact], query: '', limit: 0), isEmpty);
  });
}
