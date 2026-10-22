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
            connection_name: "orders-prod".into(),
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
    let ParsedStatement::Mongo { command, .. } = p[0].parsed() else {
        panic!()
    };
    assert!(command["filter"]["name"].is_string());
    q.text = r#"{"find":{"$param":"name"},"filter":{}}"#.into();
    assert!(g.prepare(&q).is_err());
    q.text = r#"db.items.find({"name":{"$param":"name"}})"#.into();
    q.parameters[0].value = DbCell::Json(r#"{"$ne":null}"#.into());
    assert!(g.prepare(&q).is_err());
}
fn sql_text(statement: &ParsedStatement) -> &str {
    match statement {
        ParsedStatement::Sql { text, .. } => text,
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn backslash_literals_cannot_smuggle_uninspected_sql() {
    // The server unescapes `\\` inside the literal; a verbatim rendering would
    // end the first string early and execute ` FROM url(...)` uninspected.
    for engine in [DbEngine::Clickhouse, DbEngine::Mysql, DbEngine::Mariadb] {
        let g = guard(engine, true, false);
        let text = r"SELECT 'x\\', ' FROM url(1) -- ', 'a\\nb', 'it''s'";
        let prepared = g.prepare(&request(text)).unwrap();
        assert!(prepared[0].is_read_only(), "{engine:?}");
        assert_eq!(sql_text(prepared[0].parsed()), text, "{engine:?}");
    }
    // Where a string is rendered without escaping, the mismatch fails closed.
    let g = guard(DbEngine::Clickhouse, true, false);
    assert!(g.prepare(&request(r"SHOW TABLES LIKE 'x\\'")).is_err());
    // PostgreSQL standard strings have no backslash escapes.
    let g = guard(DbEngine::Postgresql, true, false);
    let prepared = g.prepare(&request(r"SELECT 'x\'")).unwrap();
    assert_eq!(sql_text(prepared[0].parsed()), r"SELECT 'x\'");
}
#[test]
fn clickhouse_type_names_keep_their_case_sensitive_spelling() {
    let g = guard(DbEngine::Clickhouse, true, false);
    let prepared = g
        .prepare(&request(
            "SELECT CAST(1 AS String), CAST(2 AS Nullable(Float64)), CAST(3 AS Int8), \
             CAST([4] AS Array(Int64)), CAST('a' AS Enum8('a' = 1, 'b''c' = 2)), \
             CAST(5 AS Map(String, LowCardinality(String)))",
        ))
        .unwrap();
    assert_eq!(
        sql_text(prepared[0].parsed()),
        "SELECT CAST(1 AS String), CAST(2 AS Nullable(Float64)), CAST(3 AS Int8), \
         CAST([4] AS Array(Int64)), CAST('a' AS Enum8('a' = 1, 'b\\'c' = 2)), \
         CAST(5 AS Map(String, LowCardinality(String)))"
    );
    let g = guard(DbEngine::Clickhouse, false, false);
    let mut q = request(
        "CREATE TABLE t (a String, b Nullable(Float64), c Enum16('x' = 1)) \
         ENGINE = MergeTree ORDER BY a",
    );
    let review = g.review(&q).unwrap();
    g.approve(&review.ticket).unwrap();
    q.approval_ticket = Some(review.ticket);
    let prepared = g.prepare(&q).unwrap();
    let text = sql_text(prepared[0].parsed());
    assert!(
        text.starts_with("CREATE TABLE t (a String, b Nullable(Float64), c Enum16('x' = 1))"),
        "{text}"
    );
}
#[test]
fn mongo_commands_keep_written_key_order_and_refuse_duplicate_keys() {
    let g = guard(DbEngine::Mongodb, false, false);
    let order = |text: &str| {
        let p = g.prepare(&request(text)).unwrap();
        let ParsedStatement::Mongo { order, .. } = p[0].parsed() else {
            panic!()
        };
        order.clone()
    };
    let find = order(r#"db.items.find({"b":1,"a":{"z":1,"y":2}}).sort({"b":1,"a":-1})"#);
    assert_eq!(find.keys().next(), Some("find"));
    assert_eq!(
        find.child("sort").unwrap().keys().collect::<Vec<_>>(),
        ["b", "a"]
    );
    let filter = find.child("filter").unwrap();
    assert_eq!(filter.keys().collect::<Vec<_>>(), ["b", "a"]);
    assert_eq!(
        filter.child("a").unwrap().keys().collect::<Vec<_>>(),
        ["z", "y"]
    );
    let insert = order(r#"db.items.insertOne({"z":1,"a":2})"#);
    let document = insert.child("documents").unwrap().element(0).unwrap();
    assert_eq!(document.keys().collect::<Vec<_>>(), ["z", "a"]);
    let json = order(r#"{"filter":{"y":1,"x":1},"find":"items"}"#);
    assert_eq!(json.keys().collect::<Vec<_>>(), ["filter", "find"]);
    // serde_json keeps the last duplicate; refusing duplicates keeps the
    // classified command identical to the sent one.
    for query in [
        r#"{"aggregate":"items","pipeline":[{"$out":"x"}],"pipeline":[],"cursor":{}}"#,
        r#"{"aggregate":"items","pipeline":[],"pipeline":[{"$match":{}}],"cursor":{}}"#,
        r#"{"find":"items","filter":{"a":1,"a":2}}"#,
        r#"db.items.find({"a":1,"a":{"$gt":0}})"#,
        r#"db.items.find({}).sort({"a":1,"a":-1})"#,
    ] {
        assert!(
            matches!(g.prepare(&request(query)), Err(DbError::InvalidRequest)),
            "accepted {query}"
        );
    }
}

#[test]
fn production_writes_need_an_open_window_bound_to_the_connection_name() {
    use std::time::Duration;
    let five = Duration::from_secs(300);
    for engine in DbEngine::ALL {
        let query = match engine {
            DbEngine::Redis => "SET k v",
            DbEngine::Mongodb => r#"{"insert":"items","documents":[{"x":1}]}"#,
            _ => "INSERT INTO items(x) VALUES(1)",
        };
        let prod = guard(engine, true, false);
        // No window: still blocked, and nothing else opens one.
        assert!(prod.is_read_only());
        assert!(prod.allow_session_write().is_err());
        assert!(matches!(
            prod.prepare(&request(query)),
            Err(DbError::PolicyDenied)
        ));
        assert!(matches!(
            prod.review(&request(query)),
            Err(DbError::PolicyDenied)
        ));
        assert_eq!(prod.production_write_remaining(), None);
        // Only 5/15/30 minutes exist.
        for minutes in [0, 1, 10, 60, 24 * 60] {
            assert!(prod
                .open_production_write(Duration::from_secs(minutes * 60))
                .is_err());
        }
        assert!(prod.is_read_only());
        // Within the window the same write gets a permit, marked as a
        // production write; it still goes through review.
        prod.open_production_write(five).unwrap();
        assert!(!prod.is_read_only());
        assert!(prod.production_write_remaining().unwrap() <= five);
        let review = prod.review(&request(query)).unwrap();
        assert_ne!(review.risk, QueryRisk::ReadOnly);
        let permit = prod.prepare(&request(query)).unwrap();
        assert!(permit[0].production_write() && !permit[0].requires_server_read_only());
        // Locking re-locks at once and voids issued permits.
        prod.close_production_write();
        assert!(prod.is_read_only());
        assert!(prod.validate_permit(&permit[0]).is_err());
        assert!(prod.prepare(&request(query)).is_err());
        // Closing the session ends the window for good.
        prod.open_production_write(five).unwrap();
        prod.close();
        assert!(prod.is_read_only());
        assert!(prod.open_production_write(five).is_err());
        // Non-production and mobile production sessions never get a window.
        assert!(guard(engine, false, false)
            .open_production_write(five)
            .is_err());
        let mobile = guard(engine, true, true);
        assert!(mobile.open_production_write(five).is_err());
        assert!(mobile.prepare(&request(query)).is_err());
    }
}
#[test]
fn read_only_diagnostic_functions_run_on_production_and_resolve_to_builtins() {
    let g = guard(DbEngine::Postgresql, true, false);
    for (query, function) in [
        ("SELECT format('%I.%I', 'a', 'b')", "format"),
        ("SELECT version()", "version"),
        ("SELECT pg_backend_pid()", "pg_backend_pid"),
        ("SELECT pg_blocking_pids(1)", "pg_blocking_pids"),
        (
            "SELECT pg_postmaster_start_time()",
            "pg_postmaster_start_time",
        ),
        ("SELECT inet_server_addr()", "inet_server_addr"),
        ("SELECT inet_server_port()", "inet_server_port"),
        ("SELECT pg_database_size('demo')", "pg_database_size"),
        ("SELECT pg_is_in_recovery()", "pg_is_in_recovery"),
        ("SELECT pg_current_wal_lsn()", "pg_current_wal_lsn"),
        (
            "SELECT pg_last_wal_receive_lsn()",
            "pg_last_wal_receive_lsn",
        ),
        ("SELECT pg_last_wal_replay_lsn()", "pg_last_wal_replay_lsn"),
        (
            "SELECT pg_last_xact_replay_timestamp()",
            "pg_last_xact_replay_timestamp",
        ),
        ("SELECT pg_wal_lsn_diff('0/2', '0/1')", "pg_wal_lsn_diff"),
        ("SELECT col_description(1, 1)", "col_description"),
        ("SELECT obj_description(1, 'pg_class')", "obj_description"),
        ("SELECT pg_get_constraintdef(1)", "pg_get_constraintdef"),
        ("SELECT pg_get_indexdef(1)", "pg_get_indexdef"),
        ("SELECT pg_catalog.pg_backend_pid()", "pg_backend_pid"),
    ] {
        let p = g
            .prepare(&request(query))
            .unwrap_or_else(|e| panic!("{query}: {e:?}"));
        assert!(p[0].is_read_only(), "{query}");
        // Always the server's own function, never one earlier on the search_path.
        assert!(
            sql_text(p[0].parsed()).contains(&format!("pg_catalog.{function}(")),
            "{}",
            sql_text(p[0].parsed())
        );
    }
    let g = guard(DbEngine::Clickhouse, true, false);
    for query in [
        "SELECT formatReadableSize(1024)",
        "SELECT formatReadableQuantity(1000)",
        "SELECT formatReadableTimeDelta(100)",
        "SELECT uptime()",
        "SELECT version()",
        "SELECT hostName()",
        "SELECT toStartOfMinute(now())",
        "SELECT toStartOfFiveMinutes(now())",
        "SELECT toStartOfFifteenMinutes(now())",
        "SELECT toStartOfWeek(today())",
        "SELECT toStartOfQuarter(today())",
    ] {
        let p = g
            .prepare(&request(query))
            .unwrap_or_else(|e| panic!("{query}: {e:?}"));
        assert!(p[0].is_read_only(), "{query}");
    }
}
#[test]
fn side_effecting_server_functions_stay_blocked_on_production() {
    let g = guard(DbEngine::Postgresql, true, false);
    for query in [
        "SELECT pg_cancel_backend(1)",
        "SELECT pg_terminate_backend(1)",
        "SELECT pg_reload_conf()",
        "SELECT pg_rotate_logfile()",
        "SELECT set_config('work_mem', '1GB', false)",
        "SELECT nextval('s')",
        "SELECT setval('s', 1)",
        "SELECT pg_advisory_lock(1)",
        "SELECT pg_advisory_xact_lock(1)",
        "SELECT dblink_exec('x', 'x')",
        "SELECT lo_import('/etc/passwd')",
        "SELECT lo_export(1, '/tmp/x')",
        "SELECT pg_read_file('/etc/passwd')",
        "SELECT pg_ls_dir('.')",
        "SELECT pg_stat_reset()",
        "SELECT pg_stat_statements_reset()",
        "SELECT pg_switch_wal()",
        "SELECT pg_create_restore_point('x')",
        "SELECT public.pg_backend_pid()",
        "SELECT \"PG_BACKEND_PID\"()",
    ] {
        assert!(g.prepare(&request(query)).is_err(), "accepted {query}");
    }
    let g = guard(DbEngine::Clickhouse, true, false);
    for query in [
        "SELECT * FROM file('x.csv')",
        "SELECT * FROM url('http://example.com', CSV)",
        "SELECT * FROM s3('http://example.com/x', CSV)",
        "SELECT * FROM remote('h', system.one)",
        "SELECT * FROM mysql('h:3306', 'db', 't', 'u', 'p')",
        "SELECT * FROM postgresql('h:5432', 'db', 't', 'u', 'p')",
        "SELECT * FROM executable('x.sh', TabSeparated, 'a String')",
        "SELECT sleep(1)",
        "SELECT throwIf(1)",
    ] {
        assert!(g.prepare(&request(query)).is_err(), "accepted {query}");
    }
}

#[test]
fn cql_production_is_read_only_and_never_manages_roles() {
    use std::time::Duration;
    let prod = guard(DbEngine::Cassandra, true, false);
    for read in [
        "SELECT * FROM shop.items WHERE id = 1",
        "SELECT * FROM shop.items",
        "USE shop",
        "DESCRIBE KEYSPACES",
    ] {
        assert!(
            prod.prepare(&request(read)).unwrap()[0].is_read_only(),
            "{read}"
        );
    }
    for write in [
        "INSERT INTO items (id) VALUES (1)",
        "UPDATE items SET v = 1 WHERE id = 1",
        "BEGIN BATCH DELETE FROM items WHERE id = 1; APPLY BATCH",
        "CREATE TABLE t (a int PRIMARY KEY)",
        "TRUNCATE items",
    ] {
        assert!(
            matches!(prod.prepare(&request(write)), Err(DbError::PolicyDenied)),
            "{write}"
        );
    }
    prod.open_production_write(Duration::from_secs(300))
        .unwrap();
    assert!(!prod
        .prepare(&request("INSERT INTO items (id) VALUES (1)"))
        .unwrap()[0]
        .is_read_only());
    // Role management stays blocked inside the window, even with a review.
    for role in [
        "GRANT SELECT ON KEYSPACE shop TO reader",
        "CREATE ROLE reader WITH LOGIN = true",
        "DROP USER old",
    ] {
        assert!(
            matches!(prod.review(&request(role)), Err(DbError::PolicyDenied)),
            "{role}"
        );
        assert!(
            matches!(prod.prepare(&request(role)), Err(DbError::PolicyDenied)),
            "{role}"
        );
    }
    // Development: destructive statements need the exact approval.
    let dev = guard(DbEngine::Cassandra, false, false);
    dev.allow_session_write().unwrap();
    let mut drop = request("DROP TABLE items");
    assert!(matches!(dev.prepare(&drop), Err(DbError::ApprovalRequired)));
    let review = dev.review(&drop).unwrap();
    assert_eq!(review.risk, QueryRisk::Destructive);
    assert_eq!(review.reason_codes, ["cql_drop"]);
    dev.approve(&review.ticket).unwrap();
    drop.approval_ticket = Some(review.ticket);
    dev.prepare(&drop).unwrap();
    // A scan warning is a reason code of a read, not a block.
    let scan = dev
        .review(&request("SELECT * FROM items WHERE v = 1 ALLOW FILTERING"))
        .unwrap();
    assert_eq!(scan.risk, QueryRisk::ReadOnly);
    assert_eq!(scan.reason_codes, ["cql_allow_filtering"]);
    // Unknown statements are refused.
    assert!(matches!(
        dev.prepare(&request("ADD NODE 10.0.0.1")),
        Err(DbError::InvalidRequest)
    ));
}

#[test]
fn cql_parameters_are_bound_values() {
    let g = guard(DbEngine::Cassandra, true, false);
    let mut q = request("SELECT * FROM items WHERE id = {{id}} AND label = {{id}}");
    q.parameters = vec![param("id", "'; DROP TABLE items; --")];
    let p = g.prepare(&q).unwrap();
    let ParsedStatement::Cql {
        text, parameters, ..
    } = p[0].parsed()
    else {
        panic!()
    };
    assert_eq!(text, "SELECT * FROM items WHERE id = ? AND label = ?");
    assert_eq!(parameters.len(), 2);
    assert!(!text.contains("DROP"));
}
