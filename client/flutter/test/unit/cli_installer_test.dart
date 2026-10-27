import 'dart:io';

import 'package:consolecrypt/settings/cli_installer.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

typedef _Call = ({String executable, List<String> arguments, Map<String, String>? environment});

void main() {
  late Directory root;
  setUp(() => root = Directory.systemTemp.createTempSync('cc-cli-installer-'));
  tearDown(() {
    if (!Platform.isWindows) {
      for (final entity in root.listSync(recursive: true).whereType<Directory>()) {
        Process.runSync('chmod', ['755', entity.path]);
      }
    }
    root.deleteSync(recursive: true);
  });
  // The Linux and macOS installers link files with POSIX paths and modes.
  final posixOnly = Platform.isWindows ? 'POSIX links and modes; checked on macOS and Linux hosts' : null;

  File touch(String path) => File(path)
    ..createSync(recursive: true)
    ..writeAsStringSync('cli');

  group('Linux', () {
    late String bundled, home;
    CliInstaller installer({String path = '/usr/bin'}) => CliInstaller(
      executable: '${root.path}/opt/consolecrypt/consolecrypt',
      home: home,
      environment: {'PATH': path},
      platform: TargetPlatform.linux,
      run: (_, _, {environment}) => throw StateError('no process on Linux'),
    );
    setUp(() {
      bundled = touch('${root.path}/opt/consolecrypt/consolecrypt-cli').path;
      home = '${root.path}/home';
    });

    test('links the bundled command into ~/.local/bin and removes it again', () async {
      final cli = installer();
      expect((await cli.state()).installed, isFalse);
      await cli.install();
      final link = Link('$home/.local/bin/consolecrypt-cli');
      expect(link.targetSync(), bundled);
      final state = await cli.state();
      expect(state.installed, isTrue);
      expect(state.onPath, isFalse, reason: '~/.local/bin is not on this PATH');
      expect((await installer(path: '/usr/bin:$home/.local/bin').state()).onPath, isTrue);
      await cli.uninstall();
      expect(link.existsSync(), isFalse);
    });

    test('a foreign file or link of the same name is never replaced or removed', () async {
      final mine = touch('$home/.local/bin/consolecrypt-cli');
      await expectLater(
        installer().install(),
        throwsA(isA<CliInstallException>().having((e) => e.code, 'code', 'conflict')),
      );
      expect(mine.readAsStringSync(), 'cli');
      mine.deleteSync();
      final foreign = Link('$home/.local/bin/consolecrypt-cli')..createSync('/somewhere/else');
      await installer().uninstall();
      expect(foreign.targetSync(), '/somewhere/else');
    });

    test('a build without the bundled command offers nothing', () async {
      File(bundled).deleteSync();
      final state = await installer().state();
      expect(state.bundled, isNull);
      expect(state.installed, isFalse);
    });
  }, skip: posixOnly);

  group('macOS', () {
    late String bundled, systemBin;
    final calls = <_Call>[];
    var reply = ProcessResult(0, 0, '', '');
    CliInstaller installer() => CliInstaller(
      executable: '${root.path}/ConsoleCrypt.app/Contents/MacOS/ConsoleCrypt',
      systemBin: systemBin,
      environment: const {'PATH': '/usr/bin'},
      platform: TargetPlatform.macOS,
      run: (executable, arguments, {environment}) async {
        calls.add((executable: executable, arguments: arguments, environment: environment));
        return reply;
      },
    );
    setUp(() {
      bundled = touch('${root.path}/ConsoleCrypt.app/Contents/Helpers/consolecrypt-cli').path;
      systemBin = '${root.path}/usr-local-bin';
      calls.clear();
      reply = ProcessResult(0, 0, '', '');
    });

    test('a writable /usr/local/bin gets the link directly', () async {
      Directory(systemBin).createSync();
      await installer().install();
      expect(Link('$systemBin/consolecrypt-cli').targetSync(), bundled);
      expect(calls, isEmpty);
      expect((await installer().state()).installed, isTrue);
    });

    test('otherwise the system password dialog runs mkdir and ln with quoted paths', () async {
      Directory(systemBin).createSync();
      Process.runSync('chmod', ['555', systemBin]);
      await installer().install();
      final call = calls.single;
      expect(call.executable, '/usr/bin/osascript');
      expect(call.arguments.sublist(call.arguments.length - 3), [systemBin, bundled, '$systemBin/consolecrypt-cli']);
      expect(call.arguments.join(' '), contains('with administrator privileges'));
      expect(call.arguments.join(' '), contains('quoted form of item 2 of argv'));
      // Closing the password dialog is a cancel, not an error.
      reply = ProcessResult(0, 1, '', 'execution error: User canceled. (-128)');
      await expectLater(
        installer().install(),
        throwsA(isA<CliInstallException>().having((e) => e.code, 'code', 'cancelled')),
      );
    });
  }, skip: posixOnly);

  group('Windows', () {
    late String folder;
    final calls = <_Call>[];
    var userPath = r'C:\Tools;%USERPROFILE%\bin';
    CliInstaller installer() => CliInstaller(
      executable: '${root.path}/ConsoleCrypt.exe',
      platform: TargetPlatform.windows,
      run: (executable, arguments, {environment}) async {
        calls.add((executable: executable, arguments: arguments, environment: environment));
        return ProcessResult(0, 0, environment == null ? userPath : '', '');
      },
    );
    setUp(() {
      folder = '${root.path}\\bin';
      touch('$folder\\consolecrypt-cli.exe');
      calls.clear();
      userPath = r'C:\Tools;%USERPROFILE%\bin';
    });

    test('adds and removes the bin folder in the user PATH, passing it outside the script', () async {
      expect((await installer().state()).installed, isFalse);
      await installer().install();
      final write = calls.last;
      expect(write.executable, 'powershell.exe');
      expect(write.environment, {'CC_CLI_DIR': folder});
      final script = write.arguments.last;
      expect(script, contains(r'$parts += $d'));
      expect(script, isNot(contains(root.path)), reason: 'the folder is not spliced into the script');
      expect(script, contains("SetEnvironmentVariable('Path'"));
      userPath = '$userPath;${folder.toUpperCase()}\\';
      expect((await installer().state()).installed, isTrue, reason: 'case and a trailing slash do not matter');
      await installer().uninstall();
      expect(calls.last.arguments.last, isNot(contains(r'$parts += $d')));
    });
  });
}
