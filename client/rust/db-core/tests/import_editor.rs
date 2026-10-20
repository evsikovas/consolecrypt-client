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
