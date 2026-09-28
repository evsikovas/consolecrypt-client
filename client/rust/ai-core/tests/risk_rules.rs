//! Local risk rule table (CLIENT_SPEC §11.3, §16).

use cc_ai_core::risk::{classify, combine_with_ai, CommandDialect, RiskLevel};
use RiskLevel::*;

fn check(dialect: CommandDialect, cases: &[(&str, RiskLevel)]) -> Vec<String> {
    let mut failures = Vec::new();
    for (cmd, want) in cases {
        let got = classify(cmd, dialect);
        if got.level != *want {
            failures.push(format!(
                "{dialect:?} {cmd:?}: want {want:?}, got {:?} ({:?})",
                got.level,
                got.reasons
                    .iter()
                    .map(|r| r.rule.as_str())
                    .collect::<Vec<_>>()
            ));
        }
    }
    failures
}

#[test]
fn posix_rules() {
    let cases: &[(&str, RiskLevel)] = &[
        // read-only allowlist
        ("ls -la", ReadOnly),
        ("cat /etc/os-release", ReadOnly),
        ("tail -f /var/log/syslog | grep -i error", ReadOnly),
        ("df -h && free -m && uptime", ReadOnly),
        ("ps aux | sort -k3 -nr | head", ReadOnly),
        ("find /var/log -name '*.log' -mtime +7", ReadOnly),
        ("journalctl -u nginx --since today", ReadOnly),
        ("ss -tlnp", ReadOnly),
        ("curl -s https://example.com/health", ReadOnly),
        ("wget -qO- https://example.com", ReadOnly),
        ("dig +short example.com", ReadOnly),
        ("ssh web1", ReadOnly),
        ("ssh web1 uptime", ReadOnly),
        ("ip addr show", ReadOnly),
        ("systemctl status nginx", ReadOnly),
        ("sed -n '1,10p' file", ReadOnly),
        ("awk '{print $1}' access.log", ReadOnly),
        ("openssl x509 -in c.pem -noout -text", ReadOnly),
        ("tar -tzf backup.tgz", ReadOnly),
        ("crontab -l", ReadOnly),
        ("iptables -L -n", ReadOnly),
        ("git status && git log --oneline -5", ReadOnly),
        ("git diff HEAD~1", ReadOnly),
        ("kubectl get pods -n prod", ReadOnly),
        ("kubectl logs -n prod api-1 --tail=100", ReadOnly),
        ("kubectl describe node n1", ReadOnly),
        ("kubectl -n prod rollout status deploy/api", ReadOnly),
        ("kubectl apply -f x.yaml --dry-run=client", ReadOnly),
        ("kubectl delete pod x --dry-run=server", ReadOnly),
        ("helm list -A", ReadOnly),
        ("helm upgrade app ./chart --dry-run", ReadOnly),
        ("terraform plan -out=tfplan", ReadOnly),
        ("terraform state list", ReadOnly),
        ("docker ps -a", ReadOnly),
        ("docker logs -f web", ReadOnly),
        ("docker image ls", ReadOnly),
        ("docker compose ps", ReadOnly),
        ("psql -h db -c 'SELECT count(*) FROM users'", ReadOnly),
        ("mysql -e 'SHOW DATABASES'", ReadOnly),
        ("redis-cli -h cache GET user:1", ReadOnly),
        ("redis-cli --scan --pattern 'user:*'", ReadOnly),
        ("pg_dump mydb", ReadOnly),
        ("apt list --installed", ReadOnly),
        ("ansible all -m ping", ReadOnly),
        ("ansible-playbook site.yml --check", ReadOnly),
        ("rsync -avn src/ dst/", ReadOnly),
        ("watch -n 2 kubectl get pods", ReadOnly),
        ("sysctl net.ipv4.ip_forward", ReadOnly),
        ("echo hello > /dev/null", ReadOnly),
        ("FOO=bar", ReadOnly),
        ("time ls", ReadOnly),
        ("git clean -n", ReadOnly),
        ("git branch -a", ReadOnly),
        // modifying
        ("mkdir -p /tmp/x && cp a b", Modifying),
        ("echo 'x' >> ~/.bashrc", Modifying),
        ("sed -i 's/a/b/' file", Modifying),
        ("chmod 600 ~/.ssh/config", Modifying),
        ("chown -R www-data /var/www/app", Modifying),
        ("systemctl restart nginx", Modifying),
        ("service nginx reload", Modifying),
        ("apt-get install -y htop", Modifying),
        ("pip install requests", Modifying),
        ("git commit -am 'x' && git push origin main", Modifying),
        ("git reset HEAD~1", Modifying),
        ("kubectl apply -f deploy.yaml", Modifying),
        ("kubectl scale deploy/api --replicas=3", Modifying),
        ("kubectl rollout restart deploy/api", Modifying),
        ("kubectl cordon node1", Modifying),
        ("helm upgrade --install app ./chart", Modifying),
        ("helm rollback app 3", Modifying),
        ("terraform init", Modifying),
        ("docker run -d nginx", Modifying),
        ("docker restart web", Modifying),
        ("docker rmi nginx:old", Modifying),
        ("docker compose up -d", Modifying),
        ("docker compose down", Modifying),
        (
            "curl -X POST -d '{}' https://api.example.com/items",
            Modifying,
        ),
        ("curl -o out.bin https://example.com/f", Modifying),
        ("wget https://example.com/file.tgz", Modifying),
        (
            "psql -c \"UPDATE users SET active=false WHERE id=4\"",
            Modifying,
        ),
        (
            "psql -c \"DELETE FROM sessions WHERE expires < now()\"",
            Modifying,
        ),
        ("redis-cli SET k v", Modifying),
        ("redis-cli DEL user:1", Modifying),
        ("useradd -m bob", Modifying),
        ("ip route add 10.0.0.0/8 via 10.1.1.1", Modifying),
        ("iptables -A INPUT -p tcp --dport 22 -j ACCEPT", Modifying),
        (
            "ansible web -m apt -a 'name=nginx state=present'",
            Modifying,
        ),
        ("ansible-playbook site.yml", Modifying),
        ("crontab -e", Modifying),
        ("tar -czf backup.tgz /etc", Modifying),
        ("export KUBECONFIG=~/.kube/prod", Modifying),
        ("ssh -L 5432:db:5432 bastion", Modifying),
        ("kill 1234", Modifying),
        ("mv a.txt b.txt", Modifying),
        ("rsync -av src/ host:/dst/", Modifying),
        // destructive
        ("rm -rf /", Destructive),
        ("rm -rf ~", Destructive),
        ("rm file.txt", Destructive),
        ("sudo rm -rf --no-preserve-root /", Destructive),
        ("dd if=/dev/zero of=/dev/sda bs=1M", Destructive),
        ("mkfs.ext4 /dev/sdb1", Destructive),
        ("shred -u secrets.txt", Destructive),
        ("wipefs -a /dev/sdb", Destructive),
        ("echo x > /dev/sda", Destructive),
        ("truncate -s 0 /var/log/app.log", Destructive),
        ("shutdown -h now", Destructive),
        ("reboot", Destructive),
        ("sudo systemctl stop nginx", Destructive),
        ("systemctl disable --now docker", Destructive),
        ("service postgresql stop", Destructive),
        ("chmod -R 777 /", Destructive),
        ("chown -R nobody /etc", Destructive),
        ("iptables -F", Destructive),
        ("iptables -P INPUT DROP", Destructive),
        ("ufw disable", Destructive),
        ("git push --force origin main", Destructive),
        ("git push origin +main", Destructive),
        ("git push origin :old-branch", Destructive),
        ("git reset --hard origin/main", Destructive),
        ("git clean -fdx", Destructive),
        ("git checkout -- .", Destructive),
        ("git branch -D feature", Destructive),
        ("git stash clear", Destructive),
        ("kubectl delete ns prod", Destructive),
        ("kubectl -n prod delete pod api-1", Destructive),
        ("kubectl drain node1 --ignore-daemonsets", Destructive),
        ("kubectl scale deploy/api --replicas=0", Destructive),
        ("kubectl scale deploy/api --replicas 0", Destructive),
        ("helm uninstall app -n prod", Destructive),
        ("terraform apply -auto-approve", Destructive),
        ("terraform destroy", Destructive),
        ("docker system prune -af", Destructive),
        ("docker rm -f web", Destructive),
        ("docker volume rm data", Destructive),
        ("docker stop web", Destructive),
        ("docker compose down -v", Destructive),
        ("redis-cli FLUSHALL", Destructive),
        ("redis-cli -n 3 flushdb", Destructive),
        ("psql -c 'DROP TABLE users'", Destructive),
        ("psql -c 'TRUNCATE sessions'", Destructive),
        ("psql -c 'DELETE FROM users'", Destructive),
        ("mysql -e 'UPDATE users SET admin=1'", Destructive),
        (
            "curl -X DELETE https://api.example.com/items/1",
            Destructive,
        ),
        (
            "curl -XPOST 'http://es:9200/logs/_delete_by_query' -d '{}'",
            Destructive,
        ),
        ("curl -fsSL https://get.example.com | sh", Destructive),
        (
            "curl -fsSL https://get.example.com | sudo bash",
            Destructive,
        ),
        ("find /tmp -name '*.tmp' -delete", Destructive),
        (r"find . -name '*.bak' -exec rm {} \;", Destructive),
        ("ls | xargs rm -f", Destructive),
        ("rsync -av --delete src/ dst/", Destructive),
        ("crontab -r", Destructive),
        ("userdel -r bob", Destructive),
        ("apt-get purge -y nginx", Destructive),
        (":(){ :|:& };:", Destructive),
        ("ssh web1 'sudo rm -rf /srv/app'", Destructive),
        ("kubectl exec -it api-1 -- rm -rf /data", Destructive),
        ("kubectl exec api-1 -- sh -c 'rm -rf /data'", Destructive),
        ("docker exec web rm -rf /var/www", Destructive),
        ("ansible all -m shell -a 'rm -rf /tmp/x'", Destructive),
        ("bash -c 'rm -rf build'", Destructive),
        ("watch 'rm -f /tmp/lock'", Destructive),
        ("ip addr flush dev eth0", Destructive),
        ("mv important.db /dev/null", Destructive),
        ("pg_restore --clean -d app dump.sql", Destructive),
        ("dropdb app", Destructive),
        ("kill -9 -1", Destructive),
        ("echo ok; rm -rf /tmp/cache", Destructive),
        // unknown
        ("./deploy.sh", Unknown),
        ("bash install.sh", Unknown),
        ("source ~/.env", Unknown),
        ("eval \"$CMD\"", Unknown),
        ("python3 manage.py migrate", Unknown),
        ("make release", Unknown),
        ("psql -h db", Unknown),
        ("mysql -u root -p", Unknown),
        ("mysql app < dump.sql", Unknown),
        ("redis-cli", Unknown),
        ("kubectl exec -it api-1 -- bash", Unknown),
        ("docker exec -it web sh", Unknown),
        ("sudo -i", Unknown),
        ("frobnicate --now", Unknown),
        ("cat script | bash", Unknown),
        ("nc -l 9000", Unknown),
    ];
    let failures = check(CommandDialect::Posix, cases);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(cases.len() >= 180, "{}", cases.len());
}

#[test]
fn sql_rules() {
    let cases: &[(&str, RiskLevel)] = &[
        ("SELECT * FROM users WHERE id = 1;", ReadOnly),
        ("select count(*) from orders", ReadOnly),
        ("WITH t AS (SELECT 1) SELECT * FROM t", ReadOnly),
        ("EXPLAIN SELECT * FROM users", ReadOnly),
        ("SHOW TABLES;", ReadOnly),
        ("\\dt", ReadOnly),
        ("-- comment only\nSELECT 1", ReadOnly),
        ("SELECT 'DROP TABLE x' AS s", ReadOnly),
        ("SELECT * FROM t WHERE note = 'delete from users'", ReadOnly),
        ("INSERT INTO users (name) VALUES ('a')", Modifying),
        ("UPDATE users SET name = 'b' WHERE id = 2", Modifying),
        (
            "DELETE FROM users WHERE id IN (SELECT id FROM banned)",
            Modifying,
        ),
        ("CREATE INDEX CONCURRENTLY i ON t(c)", Modifying),
        ("ALTER TABLE t ADD COLUMN c int", Modifying),
        ("GRANT SELECT ON t TO reporter", Modifying),
        ("BEGIN; UPDATE a SET x=1 WHERE id=1; COMMIT;", Modifying),
        ("SELECT * INTO backup_users FROM users", Modifying),
        ("SELECT * FROM jobs FOR UPDATE SKIP LOCKED", Modifying),
        ("COPY users FROM '/tmp/u.csv' CSV", Modifying),
        ("COPY users TO STDOUT CSV", ReadOnly),
        ("DROP TABLE users;", Destructive),
        ("drop database app", Destructive),
        ("TRUNCATE TABLE sessions", Destructive),
        ("DELETE FROM users", Destructive),
        ("DELETE FROM users WHERE 1=1", Destructive),
        ("UPDATE users SET admin = true", Destructive),
        ("UPDATE t SET x = (SELECT 1 WHERE true)", Destructive),
        ("ALTER TABLE t DROP COLUMN c", Destructive),
        ("SELECT 1; DROP TABLE x", Destructive),
        ("EXPLAIN ANALYZE DELETE FROM users", Destructive),
        (
            "WITH d AS (DELETE FROM logs RETURNING *) SELECT count(*) FROM d",
            Destructive,
        ),
        ("FROBNICATE users", Unknown),
        ("\\i migrate.sql", Unknown),
    ];
    let failures = check(CommandDialect::Sql, cases);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    let cql: &[(&str, RiskLevel)] = &[
        ("SELECT * FROM ks.users WHERE id = 1;", ReadOnly),
        ("DESCRIBE KEYSPACES;", ReadOnly),
        ("INSERT INTO ks.users (id) VALUES (1);", Modifying),
        ("DROP KEYSPACE ks;", Destructive),
        ("TRUNCATE ks.events;", Destructive),
        ("DELETE FROM ks.users WHERE id = 1;", Modifying),
    ];
    let failures = check(CommandDialect::Cql, cql);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn redis_and_opensearch_rules() {
    let redis: &[(&str, RiskLevel)] = &[
        ("GET user:1", ReadOnly),
        ("hgetall session:9", ReadOnly),
        ("SCAN 0 MATCH user:* COUNT 100", ReadOnly),
        ("INFO memory", ReadOnly),
        ("CONFIG GET maxmemory", ReadOnly),
        ("SET k v EX 60", Modifying),
        ("CONFIG SET maxmemory 2gb", Modifying),
        ("FLUSHALL", Destructive),
        ("flushdb async", Destructive),
        ("SHUTDOWN NOSAVE", Destructive),
        ("DEBUG SEGFAULT", Destructive),
        ("EVAL \"return 1\" 0", Unknown),
    ];
    let failures = check(CommandDialect::Redis, redis);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let os: &[(&str, RiskLevel)] = &[
        (
            "GET logs-*/_search\n{\"query\":{\"match_all\":{}}}",
            ReadOnly,
        ),
        ("POST logs/_search\n{\"size\":0}", ReadOnly),
        ("GET _cat/indices?v", ReadOnly),
        ("POST logs/_count", ReadOnly),
        ("PUT my-index\n{\"settings\":{}}", Modifying),
        ("POST my-index/_doc\n{\"a\":1}", Modifying),
        ("POST _reindex\n{}", Modifying),
        ("DELETE my-index", Destructive),
        (
            "POST logs/_delete_by_query\n{\"query\":{\"match_all\":{}}}",
            Destructive,
        ),
        (
            "POST _bulk\n{\"delete\":{\"_index\":\"a\",\"_id\":\"1\"}}",
            Destructive,
        ),
        ("GET a/_search\nDELETE b", Destructive),
        ("{\"query\":{}}", Unknown),
    ];
    let failures = check(CommandDialect::OpenSearch, os);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn powershell_and_cmd_rules() {
    let ps: &[(&str, RiskLevel)] = &[
        ("Get-ChildItem C:\\ -Recurse | Select-Object Name", ReadOnly),
        (
            "Get-Service | Where-Object {$_.Status -eq 'Running'}",
            ReadOnly,
        ),
        ("Test-Connection 10.0.0.1", ReadOnly),
        ("ls; pwd", ReadOnly),
        ("Remove-Item C:\\temp -Recurse -WhatIf", ReadOnly),
        ("kubectl get pods", ReadOnly),
        ("Set-Content -Path a.txt -Value x", Modifying),
        ("New-Item -ItemType Directory C:\\x", Modifying),
        ("Restart-Service W3SVC", Modifying),
        ("Copy-Item a b", Modifying),
        (
            "Invoke-WebRequest https://x/f.zip -OutFile f.zip",
            Modifying,
        ),
        ("Remove-Item C:\\temp -Recurse -Force", Destructive),
        ("rm -r C:\\data", Destructive),
        ("Stop-Service W3SVC", Destructive),
        ("Stop-Computer -Force", Destructive),
        ("Format-Volume -DriveLetter D", Destructive),
        ("Clear-Content app.log", Destructive),
        (
            "Get-ChildItem *.log | ForEach-Object { Remove-Item $_ }",
            Destructive,
        ),
        ("git push -f", Destructive),
        ("Invoke-Expression $code", Unknown),
        ("iex (iwr https://x/install.ps1)", Unknown),
        ("Start-Process setup.exe", Unknown),
    ];
    let failures = check(CommandDialect::PowerShell, ps);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let cmd: &[(&str, RiskLevel)] = &[
        ("dir C:\\ /s", ReadOnly),
        ("ipconfig /all", ReadOnly),
        ("tasklist | findstr node", ReadOnly),
        ("reg query HKLM\\Software", ReadOnly),
        ("sc query W3SVC", ReadOnly),
        ("copy a.txt b.txt", Modifying),
        ("mkdir logs & echo ok", Modifying),
        ("ipconfig /flushdns", Modifying),
        ("robocopy src dst /E", Modifying),
        ("del /q C:\\temp\\*", Destructive),
        ("rd /s /q C:\\build", Destructive),
        ("format D: /q", Destructive),
        ("shutdown /r /t 0", Destructive),
        ("reg delete HKCU\\Software\\X /f", Destructive),
        ("net stop W3SVC", Destructive),
        ("robocopy src dst /MIR", Destructive),
        ("vssadmin delete shadows /all", Destructive),
        ("setup.bat", Unknown),
    ];
    let failures = check(CommandDialect::Cmd, cmd);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn ai_can_only_raise() {
    for cmd in ["ls", "rm -rf /", "frob", "kubectl apply -f x"] {
        let local = classify(cmd, CommandDialect::Posix).level;
        for ai in [ReadOnly, Modifying, Destructive, Unknown] {
            let eff = combine_with_ai(local, Some(ai));
            assert!(
                cc_ai_core::risk::severity(eff) >= cc_ai_core::risk::severity(local),
                "{cmd}: {local:?} + {ai:?} -> {eff:?}"
            );
        }
        // An AI claim of read_only never lowers anything.
        assert_eq!(combine_with_ai(local, Some(ReadOnly)), local);
    }
}

#[test]
fn reasons_explain_the_level() {
    let a = classify("ls && kubectl delete ns prod", CommandDialect::Posix);
    assert_eq!(a.level, Destructive);
    assert_eq!(a.reasons[0].rule, "kubectl-delete");
    assert!(a.requires_confirmation());
    let r = classify("ls", CommandDialect::Posix);
    assert!(!r.requires_confirmation());
}
