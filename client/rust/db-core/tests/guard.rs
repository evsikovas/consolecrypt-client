use cc_db_core::{
    guard::{DbGuard, GuardPolicy, QueryRisk},
    parser::ParsedStatement,
    types::{DbCell, DbError, DbParameter, DbQueryRequest},
};
use cc_models::{database::DbEngine, ObjectId};
use tokio_util::sync::CancellationToken;
fn guard(engine: DbEngine, production: bool, mobile: bool) -> DbGuard {
    DbGuard::new(
        GuardPolicy {
            engine,
            session_id: "s".into(),
            connection_id: ObjectId::new(),
            connection_revision: "r".into(),
            default_schema: "application".into(),
            production,
            mobile,
        },
        CancellationToken::new(),
    )
}
fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: vec![],
        approval_ticket: None,
    }
}
fn param(name: &str, text: &str) -> DbParameter {
    DbParameter {
        name: name.into(),
        value: DbCell::Text(text.into()),
        type_hint: None,
    }
}
#[test]
fn one_use_native_identity_and_epoch_revocation() {
    let a = guard(DbEngine::Postgresql, false, false);
    let b = guard(DbEngine::Postgresql, false, false);
    let q = a.prepare(&request("SELECT 1")).unwrap();
    assert!(b.validate_permit(&q[0]).is_err());
    a.validate_permit(&q[0]).unwrap();
    assert!(a.validate_permit(&q[0]).is_err());
    let q = a.prepare(&request("SELECT 1")).unwrap();
    a.revoke_session_write();
    assert!(a.validate_permit(&q[0]).is_err());
    let q = a.prepare(&request("SELECT 1")).unwrap();
    a.close();
    assert!(matches!(
        a.validate_permit(&q[0]),
        Err(DbError::SessionClosed)
    ));
}
#[test]
fn mobile_write_gate_never_overrides_production() {
    for engine in DbEngine::ALL {
        let query = match engine {
            DbEngine::Redis => "SET k v",
            DbEngine::Mongodb => r#"{"insert":"items","documents":[{"x":1}]}"#,
            _ => "INSERT INTO items(x) VALUES(1)",
        };
        let mobile = guard(engine, false, true);
        assert!(mobile.prepare(&request(query)).is_err(), "{engine:?}");
        mobile.allow_session_write().unwrap();
        assert!(!mobile.prepare(&request(query)).unwrap()[0].is_read_only());
        let prod = guard(engine, true, false);
        assert!(prod.allow_session_write().is_err());
        assert!(prod.prepare(&request(query)).is_err());
    }
}
#[test]
fn exact_destructive_approval_is_consumed_and_rejects_parameter_changes() {
    let g = guard(DbEngine::Postgresql, false, false);
    let mut q = request("DELETE FROM items");
    assert!(matches!(g.prepare(&q), Err(DbError::ApprovalRequired)));
    let r = g.review(&q).unwrap();
    assert_eq!(r.risk, QueryRisk::Destructive);
    g.approve(&r.ticket).unwrap();
    q.approval_ticket = Some(r.ticket);
    let p = g.prepare(&q).unwrap();
    g.validate_permit(&p[0]).unwrap();
    assert!(g.prepare(&q).is_err());
    let mut q = request("SELECT custom_function({{value}})");
    q.parameters.push(param("value", "original"));
    let r = g.review(&q).unwrap();
    g.approve(&r.ticket).unwrap();
    q.approval_ticket = Some(r.ticket);
    q.parameters[0] = param("value", "changed");
    assert!(g.prepare(&q).is_err());
}
#[test]
fn sql_parameters_are_values_not_interpolated_text_or_identifiers() {
    for engine in [
        DbEngine::Postgresql,
        DbEngine::Mysql,
        DbEngine::Mariadb,
        DbEngine::Clickhouse,
    ] {
        let g = guard(engine, true, false);
        let mut q =
            request("SELECT {{value}}, {{other}}, {{value}} FROM items WHERE label={{other}}");
        q.parameters = vec![
            param("value", "'; DELETE FROM items; --"),
            param("other", "秘密"),
        ];
        let p = g.prepare(&q).unwrap();
        let ParsedStatement::Sql { text, parameters } = p[0].parsed() else {
            panic!()
        };
        assert!(!text.contains("DELETE"));
        assert!(!text.contains("秘密"));
        assert_eq!(parameters.len(), 4);
        assert_eq!(parameters[0].value, q.parameters[0].value);
        assert_eq!(parameters[1].value, q.parameters[1].value);
        let mut q = request("SELECT * FROM {{table}}");
        q.parameters.push(param("table", "items"));
        assert!(g.prepare(&q).is_err(), "{engine:?}");
    }
}
#[test]
fn quoted_templates_and_conditional_comments_cannot_execute() {
    let g = guard(DbEngine::Mysql, true, false);
    let p = g.prepare(&request("SELECT '{{literal}}', 1")).unwrap();
    let ParsedStatement::Sql { text, .. } = p[0].parsed() else {
        panic!()
    };
    assert!(text.contains("{{literal}}"));
    // MySQL executes /*! ... */ bodies; the parser reads them the same way, so a
    // hidden INTO OUTFILE is seen and the statement is refused.
    assert!(g
        .prepare(&request("SELECT 1 /*! INTO OUTFILE '/tmp/exfil' */"))
        .is_err());
    let mut q = request("SELECT '{{unused}}'");
    q.parameters.push(param("unused", "secret"));
    assert!(g.prepare(&q).is_err());
}
#[test]
fn postgres_builtin_resolution_and_cte_scope_are_deterministic() {
    let g = guard(DbEngine::Postgresql, true, false);
    for (query, fragment) in [
        (
            "SELECT lower(name), count(*) FROM items GROUP BY name",
            "pg_catalog.lower",
        ),
        ("WITH X AS (SELECT * FROM items) SELECT * FROM X", "FROM X"),
        (
            "WITH x AS (SELECT * FROM x) SELECT * FROM x",
            "\"application\".x",
        ),
        (
            "WITH RECURSIVE x AS (SELECT 1 UNION ALL SELECT n+1 FROM x WHERE n<4) SELECT * FROM x",
            "FROM x",
        ),
    ] {
        let p = g.prepare(&request(query)).unwrap();
        let ParsedStatement::Sql { text, .. } = p[0].parsed() else {
            panic!()
        };
        assert!(text.contains(fragment), "{text}");
        if query.contains("WITH X") {
            assert!(!text.contains("\"application\".X"), "{text}");
        }
    }
}
#[test]
fn recursive_sql_authorization_bypasses_fail_closed() {
    let g = guard(DbEngine::Postgresql, true, false);
    for query in [
        "WITH x AS (DELETE FROM items RETURNING *) SELECT * FROM x",
        "SELECT (SELECT pg_catalog.set_config('transaction_read_only','off',false))",
        "SELECT pg_sleep(10)",
        "SELECT nextval('s')",
        "SELECT setval('s',1)",
        "SELECT pg_advisory_lock(1)",
        "SELECT public.lower('a')",
        "SELECT pg_catalog.pg_read_file('/etc/passwd')",
        "SELECT 1::public.evil",
        "SELECT 1 OPERATOR(public.+) 2",
        "SELECT * FROM dblink('x','x')",
        "EXPLAIN ANALYZE SELECT 1",
        "EXPLAIN (ANALYZE true) DELETE FROM items",
        "SET transaction_read_only=off",
        "RESET ALL",
        "BEGIN READ WRITE",
        "COMMIT",
        "DO 'x'",
        "CALL danger()",
        "COPY items TO PROGRAM 'id'",
        "SELECT * FROM items FOR UPDATE",
        "SELECT 1 INTO new_table",
    ] {
        assert!(g.prepare(&request(query)).is_err(), "accepted {query}");
    }
    let g = guard(DbEngine::Mysql, true, false);
    for query in [
        "SELECT 1 INTO OUTFILE '/tmp/x'",
        "SELECT load_file('/etc/passwd')",
        "SELECT @x := 1",
        "SELECT get_lock('x',1)",
        "SELECT sleep(10)",
        "SET GLOBAL read_only=0",
        "LOCK TABLES items WRITE",
    ] {
        assert!(g.prepare(&request(query)).is_err(), "accepted {query}");
    }
}
#[test]
fn redis_binary_binding_does_not_inject_commands_and_control_commands_are_denied() {
    let g = guard(DbEngine::Redis, true, false);
    let mut q = request("GET {{key}}");
    q.parameters.push(param("key", "x\nFLUSHALL\r\n\0"));
    let p = g.prepare(&q).unwrap();
    let ParsedStatement::Redis { argv } = p[0].parsed() else {
        panic!()
    };
    assert_eq!(argv.len(), 2);
    assert_eq!(argv[1], b"x\nFLUSHALL\r\n\0");
    for query in [
        "GET x\nFLUSHALL",
        "AUTH password",
        "CLIENT REPLY OFF",
        "ACL SETUSER x +@all",
        "CONFIG SET dir /tmp",
        "EVAL_RO 'return 1' 0",
        "XREAD BLOCK 0 STREAMS x 0",
        "BITFIELD_RO x SET i8 0 1",
        "SCAN 0 COUNT 1001",
        "SORT x STORE y",
        "KEYS *",
        "MULTI\nGET x\nEXEC",
    ] {
        assert!(g.prepare(&request(query)).is_err(), "accepted {query}");
    }
}
#[test]
fn redis_keys_is_always_refused_with_an_equivalent_scan_hint() {
    for (production, mobile) in [(true, false), (false, false), (false, true)] {
        let g = guard(DbEngine::Redis, production, mobile);
        if !production && mobile {
            g.allow_session_write().unwrap();
        }
        for text in ["KEYS *", "keys user:*", "GET a\nKEYS b*"] {
            assert!(matches!(
                g.prepare(&request(text)),
                Err(DbError::PolicyDenied)
            ));
            assert!(g.review(&request(text)).is_err());
            assert_eq!(
                g.refusal_hint(&request(text)).unwrap().code,
                "redis_keys_use_scan"
            );
        }
    }
    let g = guard(DbEngine::Redis, true, false);
    // The suggestion is a runnable, guard-approved SCAN over the same pattern,
    // including patterns that need quoting and escapes.
    for (text, pattern) in [
        ("KEYS", b"*".to_vec()),
        ("KEYS user:*", b"user:*".to_vec()),
        ("KEYS \"a b\\\"c\\\\\"", b"a b\"c\\".to_vec()),
        ("KEYS \"\\xff\\x00#\"", vec![0xff, 0x00, b'#']),
    ] {
        let hint = g.refusal_hint(&request(text)).unwrap();
        assert!(!format!("{hint:?}").contains("SCAN"));
        let permits = g.prepare(&request(&hint.suggestion)).unwrap();
        assert!(permits[0].is_read_only());
        let ParsedStatement::Redis { argv } = permits[0].parsed() else {
            panic!()
        };
        assert_eq!(argv[0], b"SCAN");
        assert_eq!(argv[3], pattern, "{text}");
    }
    assert!(g.refusal_hint(&request("GET keys")).is_none());
    assert!(guard(DbEngine::Postgresql, true, false)
        .refusal_hint(&request("KEYS *"))
        .is_none());
}
#[test]
fn mongo_recursive_code_writes_and_wire_controls_are_denied() {
    let g = guard(DbEngine::Mongodb, true, false);
    for query in [
        r#"db.items.find({"x":1}).limit(10).sort({"x":1})"#,
        r#"db.items.aggregate([{"$facet":{"a":[{"$match":{"x":1}}]}}])"#,
        r#"{"explain":{"find":"items","filter":{}},"verbosity":"queryPlanner"}"#,
    ] {
        assert!(g.prepare(&request(query)).is_ok(), "rejected {query}");
    }
    for query in [
        r#"db.items.find({"$where":"return true"})"#,
        r#"db.items.aggregate([{"$facet":{"a":[{"$out":"stolen"}]}}])"#,
        r#"db.items.aggregate([{"$lookup":{"from":"other","pipeline":[{"$merge":"stolen"}],"as":"out"}}])"#,
        r#"db.items.find({"$expr":{"$function":{"body":"evil","args":[],"lang":"js"}}})"#,
        r#"db.items.find({}); db.items.drop()"#,
        r#"db.items.find(eval("x"))"#,
        r#"{"find":"items","$db":"admin"}"#,
        r#"{"find":"items","lsid":{"id":"x"}}"#,
        r#"{"aggregate":"items","pipeline":[{"$changeStream":{}}],"cursor":{}}"#,
        r#"{"explain":{"find":"items"},"verbosity":"executionStats"}"#,
    ] {
        assert!(g.prepare(&request(query)).is_err(), "accepted {query}");
    }
    assert!(guard(DbEngine::Mongodb, false, false)
        .review(&request(r#"{"drop":"items","$db":"admin"}"#))
        .is_err());
}
#[test]
fn mongo_parameters_cannot_replace_identifiers_or_inject_operators() {
    let g = guard(DbEngine::Mongodb, true, false);
    let mut q = request(r#"db.items.find({"name":{"$param":"name"}})"#);
    q.parameters.push(param("name", r#"{"$where":"evil"}"#));
    let p = g.prepare(&q).unwrap();
    let ParsedStatement::Mongo { command } = p[0].parsed() else {
        panic!()
    };
    assert!(command["filter"]["name"].is_string());
    q.text = r#"{"find":{"$param":"name"},"filter":{}}"#.into();
    assert!(g.prepare(&q).is_err());
    q.text = r#"db.items.find({"name":{"$param":"name"}})"#.into();
    q.parameters[0].value = DbCell::Json(r#"{"$ne":null}"#.into());
    assert!(g.prepare(&q).is_err());
}
