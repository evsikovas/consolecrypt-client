//! Command → parameterized snippet conversion table.

use cc_ai_core::parameterize::parameterize;
use cc_ai_core::shell::ShellDialect;
use cc_ai_core::snippet::{values_from, RenderDialect, Template};
use std::collections::HashMap;

fn check(cmd: &str, want_template: &str, want_defaults: &[(&str, Option<&str>)]) {
    let r = parameterize(cmd, ShellDialect::Posix);
    assert_eq!(r.template, want_template, "for {cmd:?}");
    for (name, default) in want_defaults {
        let v = r
            .variables
            .iter()
            .find(|v| v.name == *name)
            .unwrap_or_else(|| panic!("{cmd:?}: missing variable {name}: {:?}", r.variables));
        assert_eq!(v.default.as_deref(), *default, "{cmd:?}: default of {name}");
    }
    // Rendering the template with the defaults reproduces a command with
    // the same meaning (non-secret parts).
    let values: HashMap<String, String> = r
        .variables
        .iter()
        .map(|v| {
            (
                v.name.clone(),
                v.default.clone().unwrap_or_else(|| "X".into()),
            )
        })
        .collect();
    Template::parse(&r.template)
        .render(&values, RenderDialect::Posix)
        .unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
}

#[test]
fn conversions() {
    check(
        "kubectl logs -n prod api-7d9f-x2x4z --tail=200 -c app",
        "kubectl logs -n {{namespace}} {{pod}} --tail={{lines}} -c {{container}}",
        &[
            ("namespace", Some("prod")),
            ("pod", Some("api-7d9f-x2x4z")),
            ("lines", Some("200")),
        ],
    );
    check(
        "kubectl -n staging rollout restart deploy/api",
        "kubectl -n {{namespace}} rollout restart deploy/{{deployment}}",
        &[("deployment", Some("api"))],
    );
    check(
        "kubectl describe pod web-0 -n shop",
        "kubectl describe pod {{pod}} -n {{namespace}}",
        &[("pod", Some("web-0"))],
    );
    check(
        "ssh -p 2222 -i ~/.ssh/prod deploy@10.1.2.3",
        "ssh -p {{port}} -i {{identity_file}} {{user}}@{{host}}",
        &[
            ("port", Some("2222")),
            ("user", Some("deploy")),
            ("host", Some("10.1.2.3")),
        ],
    );
    check(
        "docker logs -f --tail 50 web-1",
        "docker logs -f --tail {{lines}} {{container}}",
        &[("container", Some("web-1"))],
    );
    check(
        "systemctl restart nginx",
        "systemctl restart {{service}}",
        &[("service", Some("nginx"))],
    );
    check(
        "journalctl -u postgresql --since 1h -n 100",
        "journalctl -u {{service}} --since {{since}} -n {{lines}}",
        &[("since", Some("1h"))],
    );
    check(
        "helm upgrade --install shop bitnami/wordpress -n web",
        "helm upgrade --install {{release}} {{chart}} -n {{namespace}}",
        &[
            ("release", Some("shop")),
            ("chart", Some("bitnami/wordpress")),
        ],
    );
    check(
        "git checkout feature/login",
        "git checkout {{branch}}",
        &[("branch", Some("feature/login"))],
    );
    check(
        "tail -f /var/log/nginx/error.log",
        "tail -f {{file}}",
        &[("file", Some("/var/log/nginx/error.log"))],
    );
    check(
        "scp dump.sql backup@backup.corp.lan:/srv/dumps/",
        "scp dump.sql {{user}}@{{host}}:/srv/dumps/",
        &[("host", Some("backup.corp.lan"))],
    );
    check(
        "curl -s https://grafana.corp.internal/api/health",
        "curl -s https://{{host}}/api/health",
        &[("host", Some("grafana.corp.internal"))],
    );
    check(
        "redis-cli -h cache.internal -p 6380 -n 2 GET session:1",
        "redis-cli -h {{host}} -p {{port}} -n {{db}} GET session:1",
        &[("db", Some("2"))],
    );
    // Same value → same variable; different values → numbered.
    check(
        "ping -c 1 10.0.0.1 && ssh 10.0.0.1 && ssh 10.0.0.2",
        "ping -c 1 {{host}} && ssh {{host}} && ssh {{host_2}}",
        &[("host", Some("10.0.0.1")), ("host_2", Some("10.0.0.2"))],
    );
    // Existing placeholders are kept.
    check(
        "kubectl logs {{pod}} -n prod",
        "kubectl logs {{pod}} -n {{namespace}}",
        &[("pod", None)],
    );
    // Nothing variable → unchanged.
    check("ls -la && df -h", "ls -la && df -h", &[]);
}

#[test]
fn secrets_are_removed_and_never_defaults() {
    let cases = [
        "mysql -h db1 -u root -pS3cretPw1 shop",
        "sshpass -p 'S3cretPw1' ssh admin@10.0.0.5",
        "curl -H 'Authorization: Bearer S3cretPw1S3cretPw1' https://api.example.com/v1/me",
        "export DB_PASSWORD=S3cretPw1 && ./migrate",
        "psql postgres://app:S3cretPw1@db.internal:5432/app",
    ];
    for c in cases {
        let r = parameterize(c, ShellDialect::Posix);
        assert!(!r.template.contains("S3cretPw1"), "{c:?} → {}", r.template);
        assert!(r.secrets_removed >= 1, "{c:?}");
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("S3cretPw1"), "{c:?} → {json}");
        for v in &r.variables {
            if matches!(v.name.as_str(), "password" | "token") || v.name.starts_with("password_") {
                assert!(v.default.is_none(), "{c:?}: {v:?}");
            }
        }
    }
}

#[test]
fn quoted_words_and_powershell() {
    let r = parameterize("kubectl logs -n \"my ns\" 'pod one'", ShellDialect::Posix);
    assert_eq!(r.template, "kubectl logs -n {{namespace}} {{pod}}");
    assert_eq!(r.variables[0].default.as_deref(), Some("my ns"));
    let out = Template::parse(&r.template)
        .render(
            &values_from([("namespace", "my ns"), ("pod", "pod one")]),
            RenderDialect::Posix,
        )
        .unwrap();
    assert_eq!(out, "kubectl logs -n 'my ns' 'pod one'");

    let r = parameterize("kubectl get pods -n prod", ShellDialect::PowerShell);
    assert_eq!(r.template, "kubectl get pods -n {{namespace}}");
}
