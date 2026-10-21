use cc_db_core::{
    import::{parse_pgpass, parse_uri},
    parser::editor::{format_query, prepare_query, QueryScope},
    types::DbError,
};
use cc_models::database::{DbEngine, DbOptions, DbTlsMode};
use secrecy::{ExposeSecret, SecretString};
fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}
#[test]
fn uri_credentials_are_separate_and_secure_defaults_are_preserved() {
    for uri in [
        "postgresql://user:p%40ss%3Aword@db.example:5433/app?sslmode=verify-full",
        "mysql://user:p%40ss%3Aword@db.example/app?charset=utf8mb4",
        "mariadb://user:p%40ss%3Aword@db.example/app",
        "clickhouses://user:p%40ss%3Aword@db.example/app",
        "rediss://user:p%40ss%3Aword@db.example/2",
        "mongodb://user:p%40ss%3Aword@db.example/app?authSource=admin&replicaSet=main",
    ] {
        let staged = parse_uri(&secret(uri)).unwrap();
        let c = staged.connection();
        assert!(c.production);
        assert_eq!(c.tls.mode, DbTlsMode::Verify);
        assert_eq!(c.username, "user");
        assert_eq!(staged.password().unwrap().expose_secret(), "p@ss:word");
        let preview = serde_json::to_string(c).unwrap();
        assert!(!preview.contains("p@ss"));
        assert!(!preview.contains("p%40"));
        assert!(c.auth.password_secret_id.is_none());
    }
    let c = parse_uri(&secret("mongodb+srv://user:pass@db.example/app")).unwrap();
    assert!(matches!(
        c.connection().options,
        DbOptions::Mongodb { srv: true, .. }
    ));
    let c = parse_uri(&secret("postgresql://user:pass@[2001:db8::1]:5432/app")).unwrap();
    assert_eq!(c.connection().host, "2001:db8::1");
}
#[test]
fn uri_options_cannot_override_native_security_or_driver_protocol() {
    for uri in [
        "postgresql://u:p@db/app?sslmode=disable",
        "postgresql://u:p@db/app?options=-c%20default_transaction_read_only=off",
        "postgresql://u:p@db/app?sslmode=verify-full&sslmode=disable",
        "mysql://u:p@db/app?local_infile=true",
        "mysql://u:p@db/app?multiStatements=true",
        "rediss://u:p@db/0?tls=false",
        "mongodb://u:p@db/app?tlsInsecure=true",
        "mongodb://u:p@db/app?tlsAllowInvalidCertificates=true",
        "mongodb://u:p@db/app?directConnection=true",
        "mongodb://u:p@db,other/app",
        "mongodb+srv://u:p@db:27017/app",
        "mongodb+srv://u:p@127.0.0.1/app",
        "postgresql://u:p%xx@db/app",
        "postgresql://u:p@db/app#password",
        "file:///etc/passwd",
        "redis://u:p@db/not-a-number",
    ] {
        assert!(parse_uri(&secret(uri)).is_err(), "accepted {uri}");
    }
    let error = parse_uri(&secret(
        "postgresql://u:never-print-this@db/a?password=secret",
    ))
    .err()
    .unwrap();
    assert!(!error.to_string().contains("secret"));
}
#[test]
fn pgpass_handles_escaped_delimiters_and_never_creates_wildcard_connections() {
    let input=secret("# comment\nlocalhost:5432:app:alice:p\\:a\\\\ss\n*:5432:app:user:ignored\n/var/run/postgresql:5432:app:user:ignored\n");
    let imported = parse_pgpass(&input).unwrap();
    assert_eq!(imported.entries.len(), 1);
    assert_eq!(imported.skipped_wildcards, 1);
    assert_eq!(imported.skipped_local_sockets, 1);
    assert_eq!(
        imported.entries[0].password().unwrap().expose_secret(),
        "p:a\\ss"
    );
    for input in [
        "a:0:b:c:password",
        "a:5432:b:c:pass:extra",
        "a:5432:b:c:bad\\q",
        "a:5432:b:c:bad\\",
    ] {
        assert!(parse_pgpass(&secret(input)).is_err());
    }
    let large = "*:5432:db:user:pass\n".repeat(1001);
    assert!(matches!(
        parse_pgpass(&secret(&large)),
        Err(DbError::ResultLimit)
    ));
}
#[test]
fn current_statement_respects_utf16_quotes_comments_and_dollar_strings() {
    let text = "SELECT '😀;'; SELECT $$a;b$$; -- comment;\nSELECT {{last}};";
    let start = text.find("SELECT $$").unwrap();
    let units = text[..start].encode_utf16().count();
    let q = prepare_query(
        DbEngine::Postgresql,
        text,
        QueryScope::Current,
        units + 3,
        units + 3,
    )
    .unwrap();
    assert_eq!(q.text, "SELECT $$a;b$$");
    let end = text.encode_utf16().count();
    let q = prepare_query(DbEngine::Postgresql, text, QueryScope::Current, end, end).unwrap();
    assert_eq!(q.parameter_names, vec!["last"]);
    let emoji = text.find('😀').unwrap();
    let split = text[..emoji].encode_utf16().count() + 1;
    assert!(prepare_query(DbEngine::Postgresql, text, QueryScope::Selection, 0, split).is_err());
    let q = prepare_query(
        DbEngine::Postgresql,
        text,
        QueryScope::Selection,
        units,
        units + 14,
    )
    .unwrap();
    assert!(q.text.starts_with("SELECT $$"));
}
#[test]
fn scopes_keep_redis_semicolons_and_mongo_boundaries() {
    let text = "GET key;\nGET other";
    let q = prepare_query(DbEngine::Redis, text, QueryScope::Current, 0, 0).unwrap();
    assert_eq!(q.text, "GET key;");
    let text = "# '\nGET {{key}}";
    let q = prepare_query(DbEngine::Redis, text, QueryScope::Current, 8, 8).unwrap();
    assert_eq!(q.text, "GET {{key}}");
    let text = r#"db.a.find({"a":";"}); db.b.find({"b":{"$param":"value"}})"#;
    let q = prepare_query(
        DbEngine::Mongodb,
        text,
        QueryScope::Current,
        text.len(),
        text.len(),
    )
    .unwrap();
    assert!(q.text.starts_with("db.b"));
    assert_eq!(q.parameter_names, vec!["value"]);
    assert!(prepare_query(DbEngine::Mongodb, text, QueryScope::Selection, 0, 0).is_err());
}
#[test]
fn formatting_preserves_bound_templates_and_quoted_literals() {
    let sql = format_query(
        DbEngine::Postgresql,
        "select '{{literal}}', {{name}} from items where id={{id}}",
    )
    .unwrap();
    assert!(sql.contains("{{name}}"));
    assert!(sql.contains("'{{literal}}'"));
    assert!(sql.contains("{{id}}"));
    let redis = format_query(DbEngine::Redis, "GET 'key with spaces'\nGET \\xff").unwrap();
    assert!(redis.contains("\\xff"));
    let mongo = format_query(
        DbEngine::Mongodb,
        r#"db.items.find({"a":{"$param":"value"}})"#,
    )
    .unwrap();
    assert!(mongo.contains("\"find\""));
    assert!(mongo.contains("\"$param\""));
}

/// Runs "current statement" for `marked`, where `‸` marks the caret.
fn current(engine: DbEngine, marked: &str) -> String {
    let caret = marked.find('‸').expect("caret marker");
    let text = marked.replacen('‸', "", 1);
    let units = text[..caret].encode_utf16().count();
    prepare_query(engine, &text, QueryScope::Current, units, units)
        .unwrap_or_else(|error| panic!("{marked:?}: {error:?}"))
        .text
}

#[test]
fn current_statement_is_the_one_on_the_caret_line() {
    let pg = DbEngine::Postgresql;
    // Any line of a multi-line statement runs the whole statement.
    let script = "SELECT 1;\n\nSELECT id,\n       name\n  FROM items\n WHERE id > 1;\n\nSELECT 3;";
    for line in [
        "SELECT id,",
        "       name",
        "  FROM items",
        " WHERE id > 1;",
    ] {
        let at = script.find(line).unwrap() + 2;
        let marked = format!("{}‸{}", &script[..at], &script[at..]);
        assert_eq!(
            current(pg, &marked),
            "SELECT id,\n       name\n  FROM items\n WHERE id > 1"
        );
    }
    // The caret after the `;` at the end of the line still means that line.
    assert_eq!(current(pg, "SELECT 1;‸\nSELECT 2;"), "SELECT 1");
    assert_eq!(current(pg, "SELECT 1;\nSELECT 2;‸"), "SELECT 2");
    assert_eq!(current(pg, "‸SELECT 1;\nSELECT 2;"), "SELECT 1");
    assert_eq!(current(pg, "SELECT 1;\n‸SELECT 2;"), "SELECT 2");
    // Blank lines separate statements without a `;`.
    assert_eq!(
        current(pg, "SELECT 1\n\nSELECT 2‸\nFROM t"),
        "SELECT 2\nFROM t"
    );
    assert_eq!(current(pg, "SELECT 1‸\n\nSELECT 2"), "SELECT 1");
    assert_eq!(current(pg, "SELECT 1\n  \t\nSELECT ‸2"), "SELECT 2");
}

#[test]
fn current_statement_on_a_blank_line_takes_the_nearest_above_then_below() {
    let pg = DbEngine::Postgresql;
    assert_eq!(current(pg, "SELECT 1;\n\n‸\n\nSELECT 2;"), "SELECT 1");
    assert_eq!(current(pg, "SELECT 1;\n‸\nSELECT 2;"), "SELECT 1");
    assert_eq!(current(pg, "‸\n\nSELECT 2;\nSELECT 3;"), "SELECT 2");
    assert_eq!(current(pg, "SELECT 1;\n\n  ‸"), "SELECT 1");
    assert!(prepare_query(pg, "\n\n", QueryScope::Current, 1, 1).is_err());
    assert!(prepare_query(pg, "-- only a comment\n", QueryScope::Current, 0, 0).is_err());
}

#[test]
fn current_statement_picks_the_one_under_the_caret_column() {
    let pg = DbEngine::Postgresql;
    assert_eq!(current(pg, "SELECT 1; SEL‸ECT 2; SELECT 3;"), "SELECT 2");
    assert_eq!(current(pg, "SELECT ‸1; SELECT 2;"), "SELECT 1");
    assert_eq!(current(pg, "SELECT 1;‸ SELECT 2;"), "SELECT 1");
    assert_eq!(current(pg, "SELECT 1; ‸SELECT 2;"), "SELECT 2");
    assert_eq!(current(pg, "SELECT 1; SELECT 2;   ‸"), "SELECT 2");
    // A statement continuing on the next line still belongs to its first line.
    assert_eq!(current(pg, "SELECT 1; SELECT‸\n  2;"), "SELECT\n  2");
}

#[test]
fn current_statement_respects_comments_and_strings() {
    let pg = DbEngine::Postgresql;
    // `;` inside strings, quoted identifiers, dollar quotes and comments
    // never splits; blank lines inside a string or comment do not either.
    assert_eq!(
        current(pg, "SELECT 'a;\n\nb' AS \"x;y\"‸;\nSELECT 2;"),
        "SELECT 'a;\n\nb' AS \"x;y\""
    );
    assert_eq!(
        current(pg, "SELECT $$\n;\n\n$$;‸\nSELECT 2;"),
        "SELECT $$\n;\n\n$$"
    );
    assert_eq!(
        current(pg, "SELECT 1 /* ;\n\n */ + ‸1;"),
        "SELECT 1 /* ;\n\n */ + 1"
    );
    // A comment line above a statement belongs to it; the comment itself
    // is not sent.
    assert_eq!(
        current(pg, "SELECT 1;\n-- the next one‸\nSELECT 2;"),
        "SELECT 2"
    );
    // A comment after `;` on the same line stays with that statement.
    assert_eq!(current(pg, "SELECT 1; -- note‸\nSELECT 2;"), "SELECT 1");
    assert_eq!(
        current(pg, "SELECT 1 -- no semicolon‸\n\nSELECT 2"),
        "SELECT 1"
    );
    let mysql = DbEngine::Mysql;
    assert_eq!(
        current(mysql, "# note\nSELECT 'a;b'‸;\nSELECT 2;"),
        "SELECT 'a;b'"
    );
    let ch = DbEngine::Clickhouse;
    assert_eq!(
        current(ch, "SELECT 1\n\nSELECT ‸2\nFORMAT JSON"),
        "SELECT 2\nFORMAT JSON"
    );
}

#[test]
fn current_statement_follows_the_same_lines_in_redis_and_mongo() {
    let redis = DbEngine::Redis;
    assert_eq!(current(redis, "GET a\nGET ‸b\nGET c"), "GET b");
    assert_eq!(current(redis, "GET a\n\n‸\nGET c"), "GET a");
    assert_eq!(current(redis, "‸\nGET c"), "GET c");
    assert_eq!(current(redis, "# heading‸\nGET c"), "GET c");
    assert_eq!(current(redis, "SET k \"one\ntwo\"\nGET ‸k"), "GET k");
    assert_eq!(
        current(redis, "SET k \"one‸\ntwo\"\nGET k"),
        "SET k \"one\ntwo\""
    );
    let mongo = DbEngine::Mongodb;
    let script = "db.a.find({\n  \"x\": 1,\n\n  \"y\": \";\"\n})\n\ndb.b.find({})";
    assert_eq!(
        current(mongo, &script.replacen("\"y\"", "\"y‸\"", 1)),
        "db.a.find({\n  \"x\": 1,\n\n  \"y\": \";\"\n})"
    );
    assert_eq!(current(mongo, &format!("{script}‸")), "db.b.find({})");
    assert_eq!(
        current(mongo, "db.a.find({}); db.b.‸find({})"),
        "db.b.find({})"
    );
    assert_eq!(
        current(mongo, "// first\ndb.a.find({})‸\n\ndb.b.find({})"),
        "db.a.find({})"
    );
    assert_eq!(
        current(mongo, "db.a.find({})\n\n‸\n\ndb.b.find({})"),
        "db.a.find({})"
    );
}
