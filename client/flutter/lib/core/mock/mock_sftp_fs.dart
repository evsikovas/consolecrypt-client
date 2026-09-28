import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/models/sftp.dart';
import 'package:consolecrypt/core/models/sftp_browser.dart';

/// In-memory file system of the mock SFTP backend (remote hosts and the
/// "local" disk). Paths are POSIX; symlinks are followed on resolution.

int octal(String digits) => int.parse(digits, radix: 8);

enum MockFsKind { file, dir, symlink }

final class MockFsNode {
  MockFsNode.dir(
    this.name, {
    List<MockFsNode> children = const [],
    String mode = '755',
    this.owner = 'root',
    this.group = 'root',
    int ageHours = 48,
  }) : kind = MockFsKind.dir,
       _size = 4096,
       mode = octal(mode),
       modified = _ago(ageHours) {
    for (final c in children) {
      this.children[c.name] = c;
    }
  }

  MockFsNode.file(
    this.name, {
    int size = 0,
    String? text,
    Uint8List? bytes,
    String mode = '644',
    this.owner = 'root',
    this.group = 'root',
    int ageHours = 30,
  }) : kind = MockFsKind.file,
       // ignore: prefer_initializing_formals — private fields cannot be named parameters.
       _size = size,
       content = bytes ?? (text == null ? null : Uint8List.fromList(utf8.encode(text))),
       mode = octal(mode),
       modified = _ago(ageHours);

  MockFsNode.symlink(this.name, String target, {this.owner = 'root', this.group = 'root', int ageHours = 100})
    : kind = MockFsKind.symlink,
      _size = target.length,
      linkTarget = target,
      mode = octal('777'),
      modified = _ago(ageHours);

  MockFsNode._copy(MockFsNode other, this.name)
    : kind = other.kind,
      _size = other._size,
      content = other.content == null ? null : Uint8List.fromList(other.content!),
      mode = other.mode,
      owner = other.owner,
      group = other.group,
      modified = DateTime.now(),
      linkTarget = other.linkTarget {
    for (final c in other.children.values) {
      children[c.name] = MockFsNode._copy(c, c.name);
    }
  }

  static DateTime _ago(int hours) => DateTime.now().subtract(Duration(hours: hours, minutes: hours * 7 % 60));

  String name;
  final MockFsKind kind;
  int _size;

  /// File bytes when the demo defines them (Quick Look / editing); files
  /// without content are served as generated text of [size] bytes.
  Uint8List? content;
  int mode;
  String owner;
  String group;
  DateTime modified;
  String? linkTarget;
  final Map<String, MockFsNode> children = {};

  bool get isDir => kind == MockFsKind.dir;

  bool get isFile => kind == MockFsKind.file;

  int get size => content?.length ?? _size;

  set size(int value) => _size = value;

  /// Total bytes of all regular files below (a file: its size).
  int get treeSize => isDir ? children.values.fold(0, (sum, c) => sum + c.treeSize) : (isFile ? size : 0);

  MockFsNode copyNamed(String newName) => MockFsNode._copy(this, newName);

  void touch() => modified = DateTime.now();
}

const _uids = {'root': 0, 'www-data': 33, 'syslog': 104, 'adm': 4};

int _uidOf(String name) => _uids[name] ?? 1000;

/// Resolves [path] from [root]; intermediate symlinks are always followed,
/// the last component only if [followLast]. `null` if missing / dangling.
MockFsNode? mockResolve(MockFsNode root, String path, {bool followLast = true, int depth = 0}) {
  if (depth > 8) return null;
  final parts = path.split('/').where((p) => p.isNotEmpty).toList();
  var node = root;
  var current = '/';
  for (var i = 0; i < parts.length; i++) {
    if (!node.isDir) return null;
    var next = node.children[parts[i]];
    if (next == null) return null;
    final last = i == parts.length - 1;
    if (next.kind == MockFsKind.symlink && (!last || followLast)) {
      final target = next.linkTarget!;
      final absolute = target.startsWith('/') ? target : joinRemotePath(current, target);
      next = mockResolve(root, _normalize(absolute), depth: depth + 1);
      if (next == null) return null;
    }
    node = next;
    current = joinRemotePath(current, parts[i]);
  }
  return node;
}

/// `realpath`: [path] with every symlink resolved, or `null` if missing.
String? mockCanonicalPath(MockFsNode root, String path, {int depth = 0}) {
  if (depth > 8) return null;
  final parts = path.split('/').where((p) => p.isNotEmpty).toList();
  var node = root;
  var current = '/';
  for (final part in parts) {
    if (!node.isDir) return null;
    final next = node.children[part];
    if (next == null) return null;
    if (next.kind == MockFsKind.symlink) {
      final target = next.linkTarget!;
      final resolved = mockCanonicalPath(
        root,
        _normalize(target.startsWith('/') ? target : joinRemotePath(current, target)),
        depth: depth + 1,
      );
      if (resolved == null) return null;
      current = resolved;
      node = mockResolve(root, resolved)!;
    } else {
      current = joinRemotePath(current, part);
      node = next;
    }
  }
  return current;
}

String _normalize(String path) {
  final out = <String>[];
  for (final p in path.split('/')) {
    if (p.isEmpty || p == '.') continue;
    if (p == '..') {
      if (out.isNotEmpty) out.removeLast();
    } else {
      out.add(p);
    }
  }
  return '/${out.join('/')}';
}

RemoteEntryKind _kindOf(MockFsNode n) => switch (n.kind) {
  MockFsKind.dir => RemoteEntryKind.directory,
  MockFsKind.file => RemoteEntryKind.file,
  MockFsKind.symlink => RemoteEntryKind.symlink,
};

/// `lstat`-style metadata of [node] at [path] (link target resolved from [root]).
RemoteFileInfo mockInfo(MockFsNode root, MockFsNode node, String path) {
  RemoteEntryKind? targetKind;
  if (node.kind == MockFsKind.symlink) {
    final target = mockResolve(root, path);
    targetKind = target == null ? null : _kindOf(target);
  }
  return RemoteFileInfo(
    name: node.name,
    path: path,
    kind: _kindOf(node),
    size: node.size,
    permissions: node.mode,
    uid: _uidOf(node.owner),
    gid: _uidOf(node.group),
    owner: node.owner,
    group: node.group,
    modifiedAt: node.modified,
    linkTarget: node.linkTarget,
    linkTargetKind: targetKind,
  );
}

/// Legacy [FileEntry] view (SftpService.listRemote / listLocal).
FileEntry mockEntry(MockFsNode root, MockFsNode node, String path) {
  final info = mockInfo(root, node, path);
  return FileEntry(
    name: node.name,
    path: path,
    isDirectory: info.isDirectory,
    size: node.size,
    modifiedAt: node.modified,
    permissions: info.modeString,
    isSymlink: info.isSymlink,
  );
}

/// Content served for a file without demo bytes: deterministic log-style
/// lines (at most [limit] bytes).
Uint8List mockGeneratedContent(MockFsNode node, int limit) {
  final existing = node.content;
  if (existing != null) return Uint8List.sublistView(existing, 0, existing.length.clamp(0, limit));
  final want = node.size.clamp(0, limit);
  final buffer = StringBuffer();
  var i = 0;
  final start = node.modified.subtract(const Duration(hours: 6));
  while (buffer.length < want) {
    final t = start.add(Duration(seconds: i * 17)).toUtc().toIso8601String();
    final level = const ['INFO', 'INFO', 'DEBUG', 'WARN', 'INFO'][i % 5];
    buffer.writeln('$t $level [worker-${i % 4}] request id=${1000 + i} path=/api/v1/items/${i % 97} status=200');
    i++;
  }
  final bytes = utf8.encode(buffer.toString());
  return Uint8List.fromList(bytes.sublist(0, want));
}

const _kib = 1024;
const _mib = 1024 * 1024;

/// 120×72 PNG (a terminal window on a teal gradient) for image Quick Look.
final Uint8List mockLogoPng = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAHgAAABICAIAAACyfKYoAAACr0lEQVR42u3Yx24TURjF8bOChDQTJ05zQkiQ2LFixQaJJ2DFFtFL'
  'qlOcYiexneIUp/f2KOzhRfIaMJKlyPhehWNZ+iQ8Z/RfzeaMfhpdjQb3ht4pgyACK+jh98og3B/5oAwKoD8qg1A3+kkZhLqxz8og'
  'QVtB149/UQahPvFVGYQHE9+UQQH0d2UQGiaHyJ6/eKXK4vXQMDVEJlYPNK0XQA+TlW38fvum2F83fzwrVnrz6a+XxWoRmtVD4/QI'
  'mVe51PpWudT6VrkmrXk9NCZHyQTtgab10JQcIxO0G6+HpplxMp3RHmhaD82zCTJ9Y7jxemieS5CJ1QNN6wXQE2Sd8QFVFq+HlvlJ'
  'MrG68XpoSU2RidUDTeshkpomE6sbr4dIOkkmVg80rYfIQpJMrB5oWg8PF2fIxOrG6wXQs2Ri9UGzemhdmiMTqxuvh9bMPNkde69v'
  'fhYLHTSth2gmRXb3ZDiteT1Es2myf66G0JrXQzSXJmOGw2bN66Ett0BGbofKmtdD2/IiGT8fHmteD+0rS2QVPUFIrHk9tK9myCp9'
  'iDBY83qIrWbJpOzG6yG2liWTsgea1kMsnyOTsgea1kNHfplMym68HjrWV8iqV76p5Po/oGk9dG6sslX9LtceNK8XQK+xVX1i1CI0'
  'q4euzTyZ/j678Xro2sqTidUDTeuhq7BOJlYPNK2H7sIGmVjdeD10b2+SidUDTeuhZ2eLTKxuvF4AXSATqw+a1UPPboFMrB5oWg/x'
  'vW0ysbrxeojv75CJ1QNN66F3f5dMrG68HnoP9sjE6oGm9dB3uE8mVjdeL4A+IBOrD5rVQ9/RAZlYPdC0Hh4dH5KJ1Y3XC6CPlEHo'
  'PzlWBqH/9EQZhMdnp8ogQZtBn58pgzBwca4MCqAvlEEYvLxUBmHw6koZhCfX18qgPwcPfFE606LcAAAAAElFTkSuQmCC',
);

/// Demo server: a user home, a PHP web root, logs and configs.
MockFsNode mockRemoteTree(String user) {
  MockFsNode mine(MockFsNode n) => n
    ..owner = user
    ..group = user;
  MockFsNode web(MockFsNode n) => n
    ..owner = 'www-data'
    ..group = 'www-data';
  MockFsNode deep(MockFsNode n, MockFsNode Function(MockFsNode) f) {
    f(n);
    for (final c in n.children.values) {
      deep(c, f);
    }
    return n;
  }

  final home = deep(
    MockFsNode.dir(
      user,
      ageHours: 3,
      children: [
        MockFsNode.dir(
          'app',
          children: [
            MockFsNode.file('config.yml', text: _configYml, ageHours: 20),
            MockFsNode.file('docker-compose.yml', text: _dockerCompose, ageHours: 26),
            MockFsNode.dir(
              'releases',
              children: [
                MockFsNode.dir(
                  'v1.8.2',
                  ageHours: 220,
                  children: [
                    MockFsNode.file('server.js', text: _serverJs, ageHours: 220),
                    MockFsNode.file('package.json', text: _packageJson, ageHours: 220),
                  ],
                ),
                MockFsNode.dir(
                  'v1.8.3',
                  ageHours: 20,
                  children: [
                    MockFsNode.file('server.js', text: _serverJs, ageHours: 20),
                    MockFsNode.file('package.json', text: _packageJson, ageHours: 20),
                  ],
                ),
                MockFsNode.file('v1.8.2.tar.gz', size: 42 * _mib, ageHours: 220),
              ],
            ),
          ],
        ),
        MockFsNode.dir('backups', children: [MockFsNode.file('db-2026-09-25.sql.gz', size: 480 * _mib, ageHours: 10)]),
        MockFsNode.dir(
          'logs',
          children: [
            MockFsNode.file('app.log', size: 12 * _mib, ageHours: 0),
            MockFsNode.file('error.log', size: 340 * _kib, ageHours: 2),
          ],
        ),
        MockFsNode.dir(
          '.ssh',
          mode: '700',
          children: [
            MockFsNode.file('authorized_keys', text: _authorizedKeys, mode: '600', ageHours: 900),
            MockFsNode.file('config', text: _sshConfig, mode: '600', ageHours: 900),
          ],
        ),
        MockFsNode.file('.bashrc', text: _bashrc, ageHours: 400),
        MockFsNode.file('.profile', text: _profile, ageHours: 900),
        MockFsNode.symlink('current', 'app/releases/v1.8.3', ageHours: 20),
        MockFsNode.symlink('latest.log', 'logs/app.log', ageHours: 20),
        MockFsNode.symlink('old-backup', '/mnt/backup/old', ageHours: 2000),
        MockFsNode.file('deploy.sh', text: _deploySh, mode: '755', ageHours: 50),
        MockFsNode.file('notes.txt', text: _notes, ageHours: 5),
        MockFsNode.file('README.md', text: _readme, ageHours: 70),
      ],
    ),
    mine,
  );

  final html = deep(
    MockFsNode.dir(
      'html',
      mode: '775',
      ageHours: 6,
      children: [
        MockFsNode.dir(
          'app',
          children: [
            MockFsNode.file('Controller.php', text: _controllerPhp, ageHours: 60),
            MockFsNode.file('routes.php', text: _routesPhp, ageHours: 60),
          ],
        ),
        MockFsNode.dir(
          'assets',
          children: [
            MockFsNode.dir('css', children: [MockFsNode.file('site.css', text: _siteCss, ageHours: 80)]),
            MockFsNode.dir('js', children: [MockFsNode.file('app.js', text: _appJs, ageHours: 80)]),
            MockFsNode.dir(
              'img',
              children: [
                MockFsNode.file('logo.png', bytes: mockLogoPng, ageHours: 300),
                MockFsNode.file('hero.jpg', size: 218 * _kib, ageHours: 300),
              ],
            ),
          ],
        ),
        MockFsNode.dir(
          'uploads',
          mode: '775',
          children: [
            MockFsNode.file('report-2026-q3.pdf', size: 1400 * _kib, ageHours: 12),
            MockFsNode.file('export.zip', size: 8 * _mib, ageHours: 36),
          ],
        ),
        MockFsNode.dir('vendor', children: [MockFsNode.file('autoload.php', text: _autoloadPhp, ageHours: 700)]),
        MockFsNode.file('.htaccess', text: _htaccess, ageHours: 700),
        MockFsNode.file('composer.json', text: _composerJson, ageHours: 700),
        MockFsNode.file('favicon.ico', size: 15 * _kib, ageHours: 2000),
        MockFsNode.file('index.php', text: _indexPhp, ageHours: 40),
        MockFsNode.file('robots.txt', text: 'User-agent: *\nDisallow: /uploads/\n', ageHours: 2000),
        MockFsNode.file('wp-config.php', text: _wpConfig, mode: '640', ageHours: 90),
      ],
    ),
    web,
  );

  return MockFsNode.dir(
    '/',
    children: [
      MockFsNode.dir(
        'etc',
        children: [
          MockFsNode.dir(
            'nginx',
            children: [
              MockFsNode.file('nginx.conf', text: _nginxConf, ageHours: 700),
              MockFsNode.dir('sites-available', children: [MockFsNode.file('default', text: _nginxSite)]),
              MockFsNode.dir('sites-enabled', children: [MockFsNode.symlink('default', '../sites-available/default')]),
            ],
          ),
          MockFsNode.dir('ssh', children: [MockFsNode.file('sshd_config', text: _sshdConfig, ageHours: 1500)]),
          MockFsNode.file('hosts', text: '127.0.0.1 localhost\n10.10.20.11 prod-web-1\n', ageHours: 3000),
          MockFsNode.file('os-release', text: _osRelease, ageHours: 2000),
        ],
      ),
      MockFsNode.dir('home', children: [home]),
      MockFsNode.dir('tmp', mode: '1777', ageHours: 0),
      MockFsNode.dir(
        'usr',
        children: [
          MockFsNode.dir(
            'local',
            children: [
              MockFsNode.dir(
                'bin',
                children: [MockFsNode.file('backup.sh', text: _deploySh, mode: '755')],
              ),
            ],
          ),
        ],
      ),
      MockFsNode.dir(
        'var',
        children: [
          MockFsNode.dir(
            'log',
            children: [
              MockFsNode.file('syslog', size: 5 * _mib, ageHours: 0, owner: 'syslog', group: 'adm', mode: '640'),
              MockFsNode.file('auth.log', size: 800 * _kib, ageHours: 1, owner: 'syslog', group: 'adm', mode: '640'),
              MockFsNode.dir(
                'nginx',
                children: [
                  MockFsNode.file('access.log', size: 22 * _mib, ageHours: 0, owner: 'www-data', group: 'adm'),
                  MockFsNode.file('error.log', size: 90 * _kib, ageHours: 3, owner: 'www-data', group: 'adm'),
                ],
              ),
            ],
          ),
          MockFsNode.dir('www', children: [html]),
        ],
      ),
    ],
  );
}

/// The demo "local disk".
MockFsNode mockLocalTree() {
  MockFsNode demo(MockFsNode n) {
    n
      ..owner = 'demo'
      ..group = 'staff';
    for (final c in n.children.values) {
      demo(c);
    }
    return n;
  }

  return MockFsNode.dir(
    '/',
    children: [
      MockFsNode.dir(
        'Users',
        children: [
          demo(
            MockFsNode.dir(
              'demo',
              children: [
                MockFsNode.dir(
                  'Documents',
                  children: [
                    MockFsNode.file('architecture.pdf', size: 3 * _mib),
                    MockFsNode.file('runbook.md', text: _readme),
                  ],
                ),
                MockFsNode.dir(
                  'Downloads',
                  children: [MockFsNode.file('release.tar.gz', size: 24 * _mib, ageHours: 5)],
                ),
                MockFsNode.dir(
                  'Projects',
                  children: [
                    MockFsNode.dir(
                      'website',
                      children: [
                        MockFsNode.file('index.html', size: 9 * _kib),
                        MockFsNode.file('site.css', text: _siteCss),
                        MockFsNode.dir('img', children: [MockFsNode.file('logo.png', bytes: mockLogoPng)]),
                      ],
                    ),
                  ],
                ),
                MockFsNode.dir('Backups'),
                MockFsNode.file('notes.txt', text: _notes),
              ],
            ),
          ),
        ],
      ),
    ],
  );
}

// Demo file contents (no real secrets: placeholders only) ---------------------

const _configYml = '''
server:
  port: 8080
  workers: 4
database:
  host: 10.10.10.20
  name: app
  user: app
  password_file: /run/secrets/db_password
log:
  level: info
  path: /home/deploy/logs/app.log
''';

const _dockerCompose = '''
services:
  app:
    image: registry.example.net/app:1.8.3
    restart: unless-stopped
    ports: ["8080:8080"]
    volumes:
      - ./config.yml:/etc/app/config.yml:ro
''';

const _serverJs = '''
import http from "node:http";

const server = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "application/json" });
  res.end(JSON.stringify({ ok: true, path: req.url }));
});

server.listen(8080);
''';

const _packageJson = '''
{
  "name": "demo-app",
  "version": "1.8.3",
  "type": "module",
  "main": "server.js"
}
''';

const _authorizedKeys = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDEMOdemoDEMOdemoDEMOdemoDEMOdemoDEMOdemo demo@laptop\n';

const _sshConfig = '''
Host github.com
  IdentityFile ~/.ssh/id_ed25519
  IdentitiesOnly yes
''';

const _bashrc = r'''
# ~/.bashrc: executed by bash(1) for non-login shells.
case $- in
    *i*) ;;
      *) return;;
esac

HISTCONTROL=ignoreboth
HISTSIZE=5000
shopt -s histappend checkwinsize

alias ll='ls -alF'
alias la='ls -A'
export EDITOR=vim
PS1='\u@\h:\w\$ '
''';

const _profile = r'''
# ~/.profile: executed by the command interpreter for login shells.
if [ -n "$BASH_VERSION" ] && [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi
PATH="$HOME/bin:$HOME/.local/bin:$PATH"
''';

const _deploySh = r'''
#!/usr/bin/env bash
set -euo pipefail

RELEASE="${1:?usage: deploy.sh <version>}"
cd "$HOME/app"
tar -xzf "releases/$RELEASE.tar.gz" -C releases/
ln -sfn "app/releases/$RELEASE" "$HOME/current"
docker compose up -d --remove-orphans
echo "deployed $RELEASE"
''';

const _notes = '''
TODO
- rotate nginx logs weekly
- move backups to object storage
- upgrade node to 22 LTS
''';

const _readme = '''
# Demo app

Deployed with `deploy.sh <version>`; the `current` symlink points to the
active release. Logs are in `~/logs`.
''';

const _controllerPhp = r'''
<?php

namespace App;

final class Controller
{
    public function index(): string
    {
        return view('home', ['title' => 'Welcome']);
    }

    public function health(): array
    {
        return ['status' => 'ok', 'time' => time()];
    }
}
''';

const _routesPhp = r'''
<?php

use App\Controller;

return [
    'GET /'        => [Controller::class, 'index'],
    'GET /health'  => [Controller::class, 'health'],
];
''';

const _siteCss = '''
:root { --accent: #066b61; }
body { font: 16px/1.5 system-ui, sans-serif; margin: 0; }
header { background: var(--accent); color: white; padding: 1rem 2rem; }
''';

const _appJs = '''
document.addEventListener("DOMContentLoaded", () => {
  const year = document.querySelector("#year");
  if (year) year.textContent = new Date().getFullYear();
});
''';

const _autoloadPhp = r'''
<?php
spl_autoload_register(function (string $class): void {
    require __DIR__ . '/../app/' . basename(str_replace('\\', '/', $class)) . '.php';
});
''';

const _htaccess = '''
RewriteEngine On
RewriteCond %{REQUEST_FILENAME} !-f
RewriteRule ^ index.php [QSA,L]
''';

const _composerJson = '''
{
  "name": "demo/site",
  "require": { "php": ">=8.2" },
  "autoload": { "psr-4": { "App\\\\": "app/" } }
}
''';

const _indexPhp = r'''
<?php
declare(strict_types=1);

require __DIR__ . '/vendor/autoload.php';

$routes = require __DIR__ . '/app/routes.php';
$key = $_SERVER['REQUEST_METHOD'] . ' ' . parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);

[$class, $method] = $routes[$key] ?? [App\Controller::class, 'index'];
echo (new $class())->$method();
''';

const _wpConfig = r'''
<?php
// Demo configuration — placeholders only.
define('DB_NAME', 'site');
define('DB_USER', 'site');
define('DB_PASSWORD', 'change-me');
define('DB_HOST', '10.10.10.20');
define('WP_DEBUG', false);
''';

const _nginxConf = '''
user www-data;
worker_processes auto;
pid /run/nginx.pid;

events { worker_connections 1024; }

http {
    sendfile on;
    include /etc/nginx/sites-enabled/*;
}
''';

const _nginxSite = r'''
server {
    listen 80 default_server;
    root /var/www/html;
    index index.php;

    location / { try_files $uri $uri/ /index.php?$query_string; }
    location ~ \.php$ { include fastcgi_params; fastcgi_pass unix:/run/php/php8.3-fpm.sock; }
}
''';

const _sshdConfig = '''
Port 22
PermitRootLogin no
PasswordAuthentication no
Subsystem sftp /usr/lib/openssh/sftp-server
''';

const _osRelease = '''
PRETTY_NAME="Ubuntu 24.04.1 LTS"
NAME="Ubuntu"
VERSION_ID="24.04"
ID=ubuntu
''';
