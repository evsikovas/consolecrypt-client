//! Snippet rendering: user values can never break out of their slot.
//! Every rendered command is re-tokenized with the same dialect's rules and
//! the slot must contain exactly the original value, with no command
//! substitution or extra commands.

use cc_ai_core::shell::{parse, ShellDialect};
use cc_ai_core::snippet::{render_snippet, values_from, RenderDialect, RenderError, Template};
use cc_models::snippet::{RiskLevel, Snippet, SnippetSource, SnippetType, SnippetVariable};
use cc_models::ObjectId;

const NASTY: &[&str] = &[
    "plain",
    "with space",
    "it's",
    "a\"b",
    "$(reboot)",
    "`id`",
    "${HOME}",
    "x; rm -rf /",
    "a && b || c",
    "a | tee /etc/passwd",
    "> /dev/sda",
    "'; DROP TABLE users; --",
    "\\",
    "trailing\\",
    "\\'",
    "*",
    "~",
    "!event",
    "#comment",
    "-rf",
    "--help",
    "",
    "üñí © \u{2019}quote\u{2019} \u{201c}dq\u{201d}",
    "$env:PATH",
    "@(1,2)",
    "{x}",
    "a'b\"c`d$e\\f",
];

fn render(t: &str, v: &str, d: RenderDialect) -> Result<String, RenderError> {
    Template::parse(t).render(&values_from([("x", v)]), d)
}

#[test]
fn posix_slots_round_trip() {
    for v in NASTY {
        for (tpl, expect) in [
            ("printf %s {{x}}", v.to_string()),
            ("printf %s '{{x}}'", v.to_string()),
            ("printf %s \"pre {{x}} post\"", format!("pre {v} post")),
            ("printf %s --opt={{x}}", format!("--opt={v}")),
        ] {
            let out = render(tpl, v, RenderDialect::Posix).unwrap();
            let cmds = parse(&out, ShellDialect::Posix);
            assert_eq!(cmds.len(), 1, "{tpl} / {v:?} → {out}");
            let c = &cmds[0];
            assert!(c.substitutions.is_empty(), "{out}");
            assert!(c.redirects.is_empty(), "{out}");
            assert_eq!(c.words.len(), 3, "{tpl} / {v:?} → {out}");
            assert_eq!(c.words[2].value, expect, "{tpl} / {v:?} → {out}");
        }
    }
}

#[test]
fn powershell_slots_round_trip() {
    for v in NASTY {
        for (tpl, expect) in [
            ("Write-Output {{x}}", v.to_string()),
            ("Write-Output '{{x}}'", v.to_string()),
            ("Write-Output \"pre {{x}} post\"", format!("pre {v} post")),
        ] {
            let out = render(tpl, v, RenderDialect::PowerShell).unwrap();
            let cmds = parse(&out, ShellDialect::PowerShell);
            assert_eq!(cmds.len(), 1, "{tpl} / {v:?} → {out}");
            let c = &cmds[0];
            assert_eq!(c.words.len(), 2, "{tpl} / {v:?} → {out}");
            assert_eq!(c.words[1].value, expect, "{tpl} / {v:?} → {out}");
            // No live subexpression left outside quotes.
            if tpl.contains('"') {
                assert!(c.substitutions.is_empty() || !out.contains("\"$("), "{out}");
            }
        }
    }
}

#[test]
fn cmd_slots_are_safe_or_rejected() {
    for v in NASTY {
        match render("echo {{x}}", v, RenderDialect::Cmd) {
            Ok(out) => {
                let cmds = parse(&out, ShellDialect::Cmd);
                assert_eq!(cmds.len(), 1, "{v:?} → {out}");
                assert_eq!(cmds[0].words.len(), 2, "{v:?} → {out}");
                assert_eq!(cmds[0].words[1].value, *v, "{v:?} → {out}");
            }
            Err(RenderError::UnsafeValue { .. }) => {
                assert!(v.contains(['"', '%', '!']), "rejected {v:?}");
            }
            Err(e) => panic!("{v:?}: {e}"),
        }
    }
}

#[test]
fn control_characters_are_rejected_for_shells() {
    for d in [
        RenderDialect::Posix,
        RenderDialect::PowerShell,
        RenderDialect::Cmd,
    ] {
        for v in ["a\nb", "a\rb", "a\tb", "\u{1b}[31m", "a\0b"] {
            assert!(
                matches!(
                    render("echo {{x}}", v, d),
                    Err(RenderError::ControlCharacter { .. })
                ),
                "{d:?} {v:?}"
            );
        }
    }
}

#[test]
fn sql_literals_identifiers_and_rejections() {
    let unq = |s: &str| s.replace("''", "'");
    for v in ["o'reilly", "'; DROP TABLE users; --", "plain", "üñí"] {
        let out = render(
            "SELECT * FROM t WHERE name = '{{x}}'",
            v,
            RenderDialect::Postgres,
        )
        .unwrap();
        assert_eq!(out.matches('\'').count() % 2, 0, "{out}");
        let inner = out
            .trim_start_matches("SELECT * FROM t WHERE name = '")
            .trim_end_matches('\'');
        assert_eq!(unq(inner), v);
        let risk = cc_ai_core::classify(&out, cc_ai_core::CommandDialect::Sql);
        assert_eq!(
            risk.level,
            RiskLevel::ReadOnly,
            "injection must stay inside the literal: {out}"
        );
    }
    assert_eq!(
        render(
            "SELECT \"{{x}}\" FROM t",
            "we\"ird",
            RenderDialect::Postgres
        )
        .unwrap(),
        "SELECT \"we\"\"ird\" FROM t"
    );
    assert_eq!(
        render("SELECT * FROM {{x}}", "public.users", RenderDialect::Sql).unwrap(),
        "SELECT * FROM public.users"
    );
    assert_eq!(
        render("LIMIT {{x}}", "10", RenderDialect::Sql).unwrap(),
        "LIMIT 10"
    );
    assert!(matches!(
        render(
            "SELECT * FROM {{x}}",
            "users; DROP TABLE x",
            RenderDialect::Sql
        ),
        Err(RenderError::UnsafeValue { .. })
    ));
    assert!(matches!(
        render("WHERE a = '{{x}}'", "a\\", RenderDialect::Sql),
        Err(RenderError::UnsafeValue { .. })
    ));
    assert_eq!(
        render("WHERE a = '{{x}}'", "a\\", RenderDialect::Postgres).unwrap(),
        "WHERE a = 'a\\'"
    );
    assert_eq!(
        render("SELECT `{{x}}`", "a`b", RenderDialect::Sql).unwrap(),
        "SELECT `a``b`"
    );
}

#[test]
fn json_values_stay_json() {
    let tpl = r#"GET /logs/_search
{"query": {"match": {"msg": "{{x}}"}}, "size": {{n}}}"#;
    let t = Template::parse(tpl);
    for v in [
        "plain",
        "quote\" }, \"evil\": {",
        "back\\slash",
        "new\nline",
        "üñí",
    ] {
        let out = t
            .render(&values_from([("x", v), ("n", "25")]), RenderDialect::Json)
            .unwrap();
        let body = out.split_once('\n').unwrap().1;
        let parsed: serde_json::Value =
            serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"));
        assert_eq!(parsed["query"]["match"]["msg"], *v);
        assert_eq!(parsed["size"], 25);
    }
    let out = t
        .render(
            &values_from([("x", "a"), ("n", "25} , \"x\": {")]),
            RenderDialect::Json,
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(out.split_once('\n').unwrap().1).unwrap();
    assert_eq!(
        parsed["size"], "25} , \"x\": {",
        "non-numbers become JSON strings"
    );
}

#[test]
fn render_snippet_uses_form_defaults_and_dialect() {
    let now = chrono::Utc::now();
    let s = Snippet {
        package_name: None,
        catalog_id: None,
        id: ObjectId::new(),
        name: "logs".into(),
        description: String::new(),
        snippet_type: SnippetType::Kubectl,
        shell: None,
        template: "kubectl logs -n {{namespace}} {{pod}} --tail={{lines}}".into(),
        variables: vec![SnippetVariable {
            name: "lines".into(),
            description: String::new(),
            default: Some("100".into()),
            required: true,
        }],
        tags: vec![],
        risk_level: RiskLevel::ReadOnly,
        source: SnippetSource::User,
        created_by: None,
        created_at: now,
        updated_at: now,
        last_used_at: None,
        usage_count: 0,
    };
    let out = render_snippet(
        &s,
        &values_from([("namespace", "prod"), ("pod", "api-1; reboot")]),
    )
    .unwrap();
    assert_eq!(out, "kubectl logs -n prod 'api-1; reboot' --tail=100");
    assert!(render_snippet(&s, &values_from([("namespace", "prod")])).is_err());

    let mut ps = s.clone();
    ps.shell = Some("pwsh".into());
    let out = render_snippet(&ps, &values_from([("namespace", "prod"), ("pod", "a'b")])).unwrap();
    assert_eq!(out, "kubectl logs -n prod 'a''b' --tail=100");
}
