import 'package:path/path.dart' as p;
import 'package:re_highlight/languages/all.dart';

String? editorLanguage(String path) {
  final name = p.basename(path).toLowerCase();
  if (name == 'dockerfile' ||
      name.startsWith('dockerfile.') ||
      name.endsWith('.dockerfile') ||
      name == 'containerfile') {
    return 'dockerfile';
  }
  if (name == 'makefile' || name == 'gnumakefile') return 'makefile';
  if (name == 'cmakelists.txt') return 'cmake';
  if ({'.bashrc', '.zshrc', '.profile'}.contains(name)) return 'bash';
  final ext = p.extension(name).replaceFirst('.', '');
  if (const {'j2', 'jinja', 'jinja2'}.contains(ext)) return 'jinja';
  const aliases = {
    'py': 'python',
    'pyi': 'python',
    'rs': 'rust',
    'js': 'javascript',
    'mjs': 'javascript',
    'cjs': 'javascript',
    'jsx': 'javascript',
    'ts': 'typescript',
    'tsx': 'typescript',
    'mts': 'typescript',
    'cts': 'typescript',
    'kt': 'kotlin',
    'kts': 'kotlin',
    'm': 'objectivec',
    'mm': 'objectivec',
    'h': 'cpp',
    'cc': 'cpp',
    'cxx': 'cpp',
    'hpp': 'cpp',
    'hxx': 'cpp',
    'cs': 'csharp',
    'sh': 'bash',
    'zsh': 'bash',
    'ps1': 'powershell',
    'psm1': 'powershell',
    'bat': 'dos',
    'cmd': 'dos',
    'html': 'xml',
    'htm': 'xml',
    'svg': 'xml',
    'plist': 'xml',
    'entitlements': 'xml',
    'vue': 'xml',
    'svelte': 'xml',
    'yml': 'yaml',
    'toml': 'ini',
    'cfg': 'ini',
    'conf': 'ini',
    'properties': 'properties',
    'rb': 'ruby',
    'gemspec': 'ruby',
    'md': 'markdown',
    'markdown': 'markdown',
    'mk': 'makefile',
    'hs': 'haskell',
    'ex': 'elixir',
    'exs': 'elixir',
    'clj': 'clojure',
    'fs': 'fsharp',
    'fsx': 'fsharp',
    'vb': 'vbnet',
    'jl': 'julia',
    'erl': 'erlang',
    'proto': 'protobuf',
    'tex': 'latex',
    'tf': 'hcl',
    'tfvars': 'hcl',
    'pl': 'perl',
    'pm': 'perl',
  };
  final lang = aliases[ext] ?? ext;
  return builtinAllLanguages.containsKey(lang) ? lang : null;
}
