import 'package:consolecrypt/core/models/models.dart';

/// A trimmed-down stand-in for ai-core's local risk rules (CLIENT_SPEC
/// §11.3). The Rust implementation is authoritative; this exists so the UI
/// can be exercised end-to-end on mocks.
final class MockRiskRules {
  const MockRiskRules._();

  static final List<(RegExp, String)> _destructive = [
    (RegExp(r'\brm\s+(-\S*[rRf]\S*\s+)+'), '`rm -r/-f` deletes files'),
    (RegExp(r'\bfind\b.*\s-delete\b'), '`find -delete` deletes matching files'),
    (
      RegExp(r'\bdrop\s+(table|database|schema|index|keyspace)\b', caseSensitive: false),
      '`DROP` removes database objects',
    ),
    (RegExp(r'\btruncate\b', caseSensitive: false), '`TRUNCATE` empties tables'),
    (RegExp(r'\bdelete\s+from\b', caseSensitive: false), '`DELETE FROM` removes rows'),
    (RegExp(r'\bkubectl\s+delete\b'), '`kubectl delete` removes cluster resources'),
    (RegExp(r'\bhelm\s+(uninstall|delete)\b'), '`helm uninstall` removes a release'),
    (RegExp(r'\bterraform\s+destroy\b'), '`terraform destroy` removes infrastructure'),
    (RegExp(r'\bmkfs(\.\w+)?\b'), '`mkfs` formats a filesystem'),
    (RegExp(r'\bdd\s+.*\bof='), '`dd of=` overwrites a device or file'),
    (RegExp(r'\b(shutdown|reboot|poweroff|halt)\b'), 'shuts down or reboots the machine'),
    (RegExp(r'\bdocker\s+(rm|rmi|system\s+prune|volume\s+(rm|prune))\b'), 'removes Docker resources'),
    (RegExp(r'\bRemove-Item\b.*-Recurse', caseSensitive: false), '`Remove-Item -Recurse` deletes files'),
    (RegExp(r'\b(FLUSHALL|FLUSHDB)\b', caseSensitive: false), 'wipes Redis data'),
    (RegExp(r'\bgit\s+push\b.*(--force|-f)\b'), 'force-push rewrites remote history'),
    (RegExp(r'(^|\s)DELETE\s+/?\w', caseSensitive: false), 'HTTP DELETE removes data'),
    (RegExp(r'-X\s*DELETE\b'), 'HTTP DELETE removes data'),
  ];

  static final List<(RegExp, String)> _modifying = [
    (RegExp(r'\bsudo\b'), 'runs with elevated privileges'),
    (RegExp(r'\b(systemctl|service)\s+.*\b(start|stop|restart|reload|enable|disable)\b'), 'changes service state'),
    (
      RegExp(r'\bkubectl\s+(apply|create|patch|scale|rollout|edit|label|annotate|set|replace|cordon|drain)\b'),
      'changes cluster state',
    ),
    (RegExp(r'\bhelm\s+(install|upgrade|rollback)\b'), 'changes a Helm release'),
    (RegExp(r'\bterraform\s+(apply|import|taint)\b'), 'changes infrastructure'),
    (RegExp(r'\bdocker\s+(run|stop|start|restart|kill|pull|compose\s+(up|down))\b'), 'changes containers'),
    (
      RegExp(r'\b(apt|apt-get|yum|dnf|brew|pip|npm)\s+(install|remove|upgrade|update|uninstall)\b'),
      'installs or removes packages',
    ),
    (RegExp(r'\b(mv|cp|chmod|chown|ln|mkdir|touch|tee)\b'), 'changes files'),
    (RegExp(r'\bsed\s+-i\b'), 'edits files in place'),
    (RegExp(r'[^<>|&]>{1,2}\s*[\w/~.]'), 'redirects output into a file'),
    (
      RegExp(
        r'\b(insert\s+into|update\s+\w+\s+set|alter\s+table|create\s+(table|index|database|user))\b',
        caseSensitive: false,
      ),
      'changes database contents or schema',
    ),
    (RegExp(r'\b(kill|pkill|killall)\b'), 'terminates processes'),
    (RegExp(r'\bansible-playbook\b'), 'applies configuration'),
    (RegExp(r'\bgit\s+(commit|push|reset|checkout|merge|rebase)\b'), 'changes a repository'),
    (RegExp(r'(^|\s)(PUT|POST|PATCH)\s+/?\w'), 'HTTP write request'),
    (RegExp(r'-X\s*(POST|PUT|PATCH)\b'), 'HTTP write request'),
    (RegExp(r'\b(SET|DEL|EXPIRE|HSET|LPUSH)\s', caseSensitive: false), 'writes Redis keys'),
    (RegExp(r'\b(Set|New|Stop|Start|Restart)-\w+', caseSensitive: false), 'PowerShell state change'),
  ];

  static final RegExp _readOnlyStart = RegExp(
    r'^(ls|cat|less|head|tail|grep|egrep|find|df|du|free|top|htop|ps|uptime|whoami|id|hostname|uname|date|pwd|'
    r'echo|env|printenv|which|journalctl|dmesg|ss|netstat|ip|ping|dig|nslookup|curl|wc|stat|file|tree|lsblk|'
    r'sort|uniq|awk|cut|jq|kubectl\s+(get|describe|logs|top|explain|version)|helm\s+(list|status|history|get)|'
    r'docker\s+(ps|images|logs|inspect|stats)|terraform\s+(plan|show|output|validate)|systemctl\s+status|'
    r'git\s+(status|log|diff|show)|select|show|explain|describe|GET|Get-\w+|Where-Object|redis-cli\s+(GET|KEYS|INFO|SCAN))\b',
    caseSensitive: false,
  );

  static RiskAssessment assess(String command, {required RiskLevel declared, required SnippetSource source}) {
    final reasons = <String>[];
    var local = RiskLevel.readOnly;
    for (final (re, why) in _destructive) {
      if (re.hasMatch(command)) {
        reasons.add(why);
        local = RiskLevel.destructive;
      }
    }
    if (local != RiskLevel.destructive) {
      for (final (re, why) in _modifying) {
        if (re.hasMatch(command)) {
          reasons.add(why);
          local = RiskLevel.modifying;
        }
      }
    }
    if (local == RiskLevel.readOnly) {
      final segments = command.split(RegExp(r'\|\||&&|[|;]')).map((s) => s.trim()).where((s) => s.isNotEmpty);
      if (segments.isEmpty || !segments.every(_readOnlyStart.hasMatch)) {
        local = RiskLevel.unknown;
        reasons.add('not recognised by local rules');
      }
    }
    return RiskAssessment(
      effective: combineRisk(declared: declared, local: local, source: source),
      local: local,
      declared: declared,
      reasons: reasons,
    );
  }
}
