//! Opt-in acceptance against a disposable local Redis:
//! CC_DB_LIVE_REDIS=host:port:user:password:db, e.g. with
//! `docker run -d --name cc-db-redis -p 127.0.0.1:56379:6379 redis:7.4-alpine
//! redis-server --requirepass cc-test-redis` →
//! CC_DB_LIVE_REDIS=127.0.0.1:56379:default:cc-test-redis:0
//! Every key it writes lives below `cc:live:` and is removed at the end.
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_db_core::{
    transport::{DbCredentials, DirectConnector},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbServerReadOnly,
    DbSession, DbTreeNode,
};
use cc_models::database::{DbConnection, DbEngine, DbOptions, DbTlsMode};
use secrecy::SecretString;
use std::{collections::BTreeSet, sync::Arc};

const BULK: usize = 650;

fn target(production: bool, database: Option<&str>) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var("CC_DB_LIVE_REDIS").ok()?;
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Redis);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = database.unwrap_or(parts[4]).into();
    connection.production = production;
    connection.tls.mode = DbTlsMode::Disable;
    Some((
        connection,
        DbCredentials {
            password: Some(SecretString::from(parts[3].to_owned())),
            client_key_pem: None,
        },
    ))
}

async fn open(id: &str, connection: &DbConnection, credentials: &DbCredentials) -> Arc<DbSession> {
    Arc::new(
        DbSession::open(
            id.into(),
            connection,
            credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    )
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

struct Output {
    columns: Vec<String>,
    rows: Vec<Vec<DbCell>>,
}

impl Output {
    fn text(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| match cell {
                        DbCell::Null => "NULL".into(),
                        DbCell::Bool(value) => value.to_string(),
                        DbCell::Int(v)
                        | DbCell::Uint(v)
                        | DbCell::Decimal(v)
                        | DbCell::Float(v)
                        | DbCell::Text(v)
                        | DbCell::Json(v)
                        | DbCell::Binary(v) => v.clone(),
                        _ => "?".into(),
                    })
                    .collect()
            })
            .collect()
    }
}

/// Writes go through the native review: ticket, explicit approval, execution.
async fn write(session: &Arc<DbSession>, text: &str) -> Result<Output, String> {
    let review = session
        .guard()
        .review(&request(text))
        .map_err(|error| format!("{error:?}"))?;
    session
        .guard()
        .approve(&review.ticket)
        .map_err(|error| format!("{error:?}"))?;
    let mut approved = request(text);
    approved.approval_ticket = Some(review.ticket.clone());
    consume(session, &approved).await
}

async fn run(session: &Arc<DbSession>, text: &str) -> Result<Output, String> {
    consume(session, &request(text)).await
}

/// The last statement's result (scripts report each statement in order).
async fn consume(session: &Arc<DbSession>, query: &DbQueryRequest) -> Result<Output, String> {
    let mut stream = session
        .execute(query)
        .map_err(|error| format!("{error:?}"))?;
    let mut output = Output {
        columns: Vec::new(),
        rows: Vec::new(),
    };
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Started { columns, .. } => {
                output.columns = columns.into_iter().map(|column| column.name).collect();
                output.rows.clear();
            }
            DbQueryEvent::Rows { rows, .. } => output.rows.extend(rows),
            DbQueryEvent::Paused { .. } => stream.control().load_more(1000).unwrap(),
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok(output)
}

async fn children(session: &DbSession, parent: Option<&DbObjectRef>) -> (Vec<DbTreeNode>, usize) {
    let mut nodes = Vec::new();
    let mut pages = 0;
    let mut cursor = None;
    loop {
        let page = session.tree(parent, cursor.as_deref()).await.unwrap();
        pages += 1;
        nodes.extend(page.nodes);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
        assert!(pages < 100, "SCAN paging does not terminate");
    }
    (nodes, pages)
}

fn find<'a>(nodes: &'a [DbTreeNode], name: &str) -> &'a DbTreeNode {
    nodes
        .iter()
        .find(|node| node.reference.name == name)
        .unwrap_or_else(|| panic!("missing tree node {name}"))
}

async fn seed(dev: &Arc<DbSession>) {
    for chunk in (0..BULK).collect::<Vec<_>>().chunks(400) {
        let pairs: Vec<String> = chunk
            .iter()
            .map(|index| format!("cc:live:bulk:{index:04} v{index}"))
            .collect();
        write(dev, &format!("MSET {}", pairs.join(" ")))
            .await
            .unwrap();
    }
    write(
        dev,
        "HSET cc:live:user:1 name alpha secret_token t1\n\
         SET cc:live:user:2:session s1 EX 3600\n\
         ZADD cc:live:scores 1.5 a 2 b 3 c\n\
         XADD cc:live:events 1-1 kind created id 7\n\
         SET cc:live:bin \"\\xff\\x00\\x80\"\n\
         SET cc:live:glob*[x] g\n\
         RPUSH cc:live:list one two three",
    )
    .await
    .unwrap();
}

/// Type-aware first entries and the description of a key.
async fn object_queries(dev: &Arc<DbSession>) {
    let key = |name: &str| DbObjectRef {
        database: "0".into(),
        schema: None,
        kind: cc_db_core::DbObjectKind::RedisKey,
        name: name.into(),
        parent_name: None,
    };
    write(dev, "HSET \"cc:obj:h\" a 1").await.unwrap();
    write(dev, "SET \"cc:obj:s\" x").await.unwrap();
    assert_eq!(
        dev.object_query(&key("cc:obj:h"), 10).await.unwrap(),
        "HSCAN cc:obj:h 0 COUNT 10"
    );
    assert_eq!(
        dev.object_query(&key("cc:obj:s"), 10).await.unwrap(),
        "GET cc:obj:s"
    );
    let folder = DbObjectRef {
        database: "0".into(),
        schema: None,
        kind: cc_db_core::DbObjectKind::RedisDatabase,
        name: "cc:obj:".into(),
        parent_name: Some("cc:".into()),
    };
    assert_eq!(
        dev.object_query(&folder, 10).await.unwrap(),
        "SCAN 0 MATCH cc:obj:* COUNT 10"
    );
    assert!(dev.ddl(&key("cc:obj:h")).await.unwrap().contains("hash"));
    write(dev, "DEL \"cc:obj:h\" \"cc:obj:s\"").await.unwrap();
}

async fn cleanup(dev: &Arc<DbSession>) {
    for chunk in (0..BULK).collect::<Vec<_>>().chunks(400) {
        let keys: Vec<String> = chunk
            .iter()
            .map(|index| format!("cc:live:bulk:{index:04}"))
            .collect();
        write(dev, &format!("UNLINK {}", keys.join(" ")))
            .await
            .unwrap();
    }
    write(
        dev,
        "DEL cc:live:user:1 cc:live:user:2:session cc:live:scores cc:live:events cc:live:bin cc:live:glob*[x] cc:live:list cc:live:copy",
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn redis_production_reads_development_writes_and_scan_tree() {
    let Some((connection, credentials)) = target(false, None) else {
        eprintln!("CC_DB_LIVE_REDIS not set; skipped");
        return;
    };
    let dev = open("dev", &connection, &credentials).await;
    assert!(!dev.info().read_only);
    assert!(!dev.info().server_version.is_empty());
    seed(&dev).await;

    // ---- Production: guarded reads only.
    let (production, credentials) = target(true, None).unwrap();
    let prod = open("prod", &production, &credentials).await;
    assert!(prod.info().read_only && prod.info().production);
    assert_eq!(prod.info().server_read_only, DbServerReadOnly::Unverified);

    let get = run(&prod, "GET cc:live:bulk:0007").await.unwrap();
    assert_eq!(get.columns, ["value"]);
    assert_eq!(get.text(), [["v7"]]);

    let hash = run(&prod, "HGETALL cc:live:user:1").await.unwrap();
    assert_eq!(hash.columns, ["field", "value"]);
    assert_eq!(hash.text(), [["name", "alpha"], ["secret_token", "t1"]]);

    let scores = run(&prod, "ZRANGE cc:live:scores 0 -1 WITHSCORES")
        .await
        .unwrap();
    assert_eq!(scores.columns, ["member", "score"]);
    assert_eq!(scores.text(), [["a", "1.5"], ["b", "2"], ["c", "3"]]);
    assert!(matches!(scores.rows[0][1], DbCell::Float(_)));

    let scan = run(&prod, "SCAN 0 MATCH cc:live:user:* COUNT 1000")
        .await
        .unwrap();
    assert_eq!(scan.columns, ["key", "next_cursor"]);
    let keys: BTreeSet<String> = scan.text().into_iter().map(|row| row[0].clone()).collect();
    assert!(keys.contains("cc:live:user:1") || scan.text()[0][1] != "0");

    assert_eq!(
        run(&prod, "TYPE cc:live:user:1").await.unwrap().text(),
        [["hash"]]
    );
    assert_eq!(
        run(&prod, "TTL cc:live:user:1").await.unwrap().text(),
        [["-1"]]
    );
    let ttl: i64 = run(&prod, "TTL cc:live:user:2:session")
        .await
        .unwrap()
        .text()[0][0]
        .parse()
        .unwrap();
    assert!(ttl > 3000);

    let list = run(&prod, "LRANGE cc:live:list 0 -1").await.unwrap();
    assert_eq!(list.columns, ["index", "value"]);
    assert_eq!(list.text(), [["0", "one"], ["1", "two"], ["2", "three"]]);

    let events = run(&prod, "XRANGE cc:live:events - +").await.unwrap();
    assert_eq!(events.columns, ["id", "fields"]);
    assert_eq!(events.text(), [["1-1", r#"{"id":"7","kind":"created"}"#]]);

    // Binary-safe: non-UTF-8 bytes arrive as base64 Binary cells.
    let binary = run(&prod, "GET cc:live:bin").await.unwrap();
    assert_eq!(
        binary.rows[0][0],
        DbCell::Binary(STANDARD.encode([0xff, 0x00, 0x80]))
    );

    // The client guard refuses writes, scripts, transactions and admin commands.
    for text in [
        "SET cc:live:bulk:0001 changed",
        "DEL cc:live:bulk:0001",
        "FLUSHALL",
        "EVAL \"return 1\" 0",
        "MULTI",
        "CONFIG SET maxmemory 1",
        "KEYS *",
    ] {
        assert!(
            matches!(
                prod.execute(&request(text)),
                Err(DbError::PolicyDenied | DbError::ApprovalRequired | DbError::InvalidRequest)
            ),
            "production accepted {text}"
        );
        assert!(prod.guard().review(&request(text)).is_err(), "{text}");
    }
    // KEYS is replaced by a runnable SCAN suggestion.
    let hint = prod
        .guard()
        .refusal_hint(&request("KEYS cc:live:user:*"))
        .unwrap();
    assert_eq!(hint.code, "redis_keys_use_scan");
    assert_eq!(hint.suggestion, "SCAN 0 MATCH cc:live:user:* COUNT 100");
    run(&prod, &hint.suggestion).await.unwrap();
    assert!(prod.guard().refusal_hint(&request("GET x")).is_none());
    assert_eq!(
        run(&prod, "GET cc:live:bulk:0001").await.unwrap().text(),
        [["v1"]]
    );

    // A cancelled query does not wedge the session.
    let stream = prod
        .execute(&request("SCAN 0 MATCH cc:live:* COUNT 1000"))
        .unwrap();
    stream.cancel();
    drop(stream);
    assert_eq!(run(&prod, "PING").await.unwrap().text(), [["PONG"]]);

    // ---- Structure: db0 … db15 → folders → keys, paged with SCAN.
    let (databases, _) = children(&prod, None).await;
    assert!(databases.len() >= 16);
    let db = find(&databases, &format!("db{}", connection.database));
    assert_eq!(db.reference.kind, DbObjectKind::RedisDatabase);
    assert!(db.has_children);
    assert!(db
        .detail
        .as_deref()
        .is_some_and(|detail| detail.ends_with("keys")));

    let (top, _) = children(&prod, Some(&db.reference)).await;
    let cc = find(&top, "cc:");
    assert!(cc.has_children);
    let (below_cc, _) = children(&prod, Some(&cc.reference)).await;
    let live = find(&below_cc, "cc:live:");
    let (below_live, _) = children(&prod, Some(&live.reference)).await;
    for folder in ["cc:live:bulk:", "cc:live:user:"] {
        let node = find(&below_live, folder);
        assert!(node.has_children);
        assert_eq!(node.reference.kind, DbObjectKind::RedisDatabase);
        assert_eq!(node.reference.parent_name.as_deref(), Some("cc:live:"));
    }
    for (key, kind) in [
        ("cc:live:scores", "zset"),
        ("cc:live:events", "stream"),
        ("cc:live:bin", "string"),
        ("cc:live:glob*[x]", "string"),
        ("cc:live:list", "list"),
    ] {
        let node = find(&below_live, key);
        assert_eq!(node.reference.kind, DbObjectKind::RedisKey);
        assert!(!node.has_children);
        assert_eq!(node.detail.as_deref(), Some(kind), "{key}");
    }
    let (users, _) = children(&prod, Some(&find(&below_live, "cc:live:user:").reference)).await;
    assert_eq!(
        find(&users, "cc:live:user:1").detail.as_deref(),
        Some("hash")
    );
    let (sessions, _) = children(&prod, Some(&find(&users, "cc:live:user:2:").reference)).await;
    assert!(find(&sessions, "cc:live:user:2:session")
        .detail
        .as_deref()
        .is_some_and(|detail| detail.starts_with("string · TTL")));

    // More than one page of keys: every key appears exactly once overall.
    let bulk = find(&below_live, "cc:live:bulk:").reference.clone();
    let first = prod.tree(Some(&bulk), None).await.unwrap();
    let next = first
        .next_cursor
        .clone()
        .expect("650 keys need a second page");
    assert!(prod.tree(Some(&live.reference), Some(&next)).await.is_err());
    let (keys, pages) = children(&prod, Some(&bulk)).await;
    assert!(pages >= 2, "{pages} pages");
    let names: BTreeSet<_> = keys
        .iter()
        .map(|node| node.reference.name.clone())
        .collect();
    assert_eq!(names.len(), BULK);
    assert!(keys
        .iter()
        .all(|node| node.reference.kind == DbObjectKind::RedisKey));

    // ---- Development: review flow, approval for destructive scripts.
    assert!(matches!(
        dev.execute(&request("EVAL \"return 1\" 0")),
        Err(DbError::ApprovalRequired)
    ));
    assert_eq!(
        write(&dev, "EVAL \"return 1\" 0").await.unwrap().text(),
        [["1"]]
    );
    assert_eq!(
        write(&dev, "SET cc:live:bulk:0001 changed")
            .await
            .unwrap()
            .text(),
        [["OK"]]
    );
    assert_eq!(
        run(&prod, "GET cc:live:bulk:0001").await.unwrap().text(),
        [["changed"]]
    );
    assert!(dev.guard().prepare(&request("KEYS *")).is_err());

    // Browsing another database keeps the query connection on its own one.
    let other = if connection.database == "1" { "2" } else { "1" };
    write(
        &dev,
        &format!("COPY cc:live:user:1 cc:live:copy DB {other} REPLACE"),
    )
    .await
    .unwrap();
    let other_db = find(&databases, &format!("db{other}")).reference.clone();
    let (other_top, _) = children(&dev, Some(&other_db)).await;
    find(&other_top, "cc:");
    assert_eq!(
        run(&dev, "EXISTS cc:live:copy").await.unwrap().text(),
        [["0"]]
    );
    assert_eq!(
        run(&dev, "EXISTS cc:live:user:1").await.unwrap().text(),
        [["1"]]
    );
    let (other_connection, credentials) = target(false, Some(other)).unwrap();
    let other_session = open("other", &other_connection, &credentials).await;
    write(&other_session, "DEL cc:live:copy").await.unwrap();
    other_session.close();

    // RESP3 renders the same tables.
    let (mut resp3, credentials) = target(true, None).unwrap();
    resp3.options = DbOptions::Redis { resp3: true };
    let resp3 = open("resp3", &resp3, &credentials).await;
    assert_eq!(
        run(&resp3, "HGETALL cc:live:user:1").await.unwrap().text(),
        hash.text()
    );
    let resp3_scores = run(&resp3, "ZRANGE cc:live:scores 0 -1 WITHSCORES")
        .await
        .unwrap();
    assert_eq!(resp3_scores.columns, ["member", "score"]);
    assert_eq!(resp3_scores.text(), [["a", "1.5"], ["b", "2"], ["c", "3"]]);
    resp3.close();

    object_queries(&dev).await;
    cleanup(&dev).await;
    prod.close();
    dev.close();
}

#[tokio::test]
async fn redis_wrong_password_is_an_authentication_failure() {
    let Some((connection, _)) = target(true, None) else {
        eprintln!("CC_DB_LIVE_REDIS not set; skipped");
        return;
    };
    let credentials = DbCredentials {
        password: Some(SecretString::from("canary-wrong-password".to_owned())),
        client_key_pem: None,
    };
    let error = DbSession::open(
        "bad".into(),
        &connection,
        &credentials,
        Arc::new(DirectConnector),
        false,
    )
    .await
    .unwrap_err();
    assert_eq!(error, DbError::AuthenticationFailed);
    assert!(!format!("{error:?} {error}").contains("canary"));
}

/// Optional: CC_DB_LIVE_REDIS_READER=host:port:user:password:db for an ACL
/// user limited to `+@read` (no CONFIG): reads work, the server refuses
/// writes even past the client guard, and the tree falls back to 16 databases.
#[tokio::test]
async fn redis_acl_read_only_user() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_REDIS_READER") else {
        eprintln!("CC_DB_LIVE_REDIS_READER not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let mut connection = DbConnection::new("reader", parts[0], DbEngine::Redis);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = parts[4].into();
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
    let credentials = DbCredentials {
        password: Some(SecretString::from(parts[3].to_owned())),
        client_key_pem: None,
    };
    let session = open("reader", &connection, &credentials).await;
    assert_eq!(
        run(&session, "EXISTS cc:live:none").await.unwrap().text(),
        [["0"]]
    );
    // Server ACL: a reviewed development write still fails with NOPERM.
    assert_eq!(
        write(&session, "SET cc:live:acl x").await.err().as_deref(),
        Some("policy_denied")
    );
    let (databases, _) = children(&session, None).await;
    assert_eq!(databases.len(), 16);
    session.close();
}
