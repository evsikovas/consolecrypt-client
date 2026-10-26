//! Opt-in acceptance against disposable local ScyllaDB and Cassandra nodes:
//! CC_DB_LIVE_SCYLLA / CC_DB_LIVE_CASSANDRA = host:port[:user:password], e.g.
//! `docker run -d --name cc-db-scylla -p 127.0.0.1:59042:9042
//! scylladb/scylla:6.2.3 --smp 1 --memory 512M --overprovisioned 1
//! --developer-mode 1 --authenticator PasswordAuthenticator` →
//! CC_DB_LIVE_SCYLLA=127.0.0.1:59042:cassandra:cassandra, and
//! `docker run -d --name cc-db-cassandra -p 127.0.0.1:59142:9042
//! -e MAX_HEAP_SIZE=512M -e HEAP_NEWSIZE=128M -e CASSANDRA_DC=dc1
//! cassandra:5.0.4` → CC_DB_LIVE_CASSANDRA=127.0.0.1:59142.
//! Everything is written to the keyspace `cc_live`, dropped at the end.
use async_trait::async_trait;
use cc_db_core::{
    edit::DbCellEdit,
    transport::{DbConnector, DbCredentials, DbDestination, DbIo, DbPath, DirectConnector},
    DbCell, DbError, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbServerReadOnly,
    DbSession, DbTransportSecurity,
};
use cc_models::database::{CqlConsistency, DbConnection, DbEngine, DbOptions, DbTlsMode};
use secrecy::SecretString;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const ROWS: usize = 1200;

fn target(variable: &str, production: bool) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(variable).ok()?;
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Cassandra);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts.get(2).copied().unwrap_or_default().into();
    connection.production = production;
    connection.tls.mode = DbTlsMode::Disable;
    connection.limits.max_rows = 1000;
    connection.options = DbOptions::Cassandra {
        contact_points: Vec::new(),
        local_datacenter: None,
        read_consistency: CqlConsistency::LocalOne,
        write_consistency: CqlConsistency::LocalQuorum,
        page_size: 100,
    };
    connection.validate().unwrap();
    Some((
        connection,
        DbCredentials {
            password: parts
                .get(3)
                .map(|password| SecretString::from((*password).to_owned())),
            client_key_pem: None,
        },
    ))
}

/// A direct socket reported as the network (as if the server were remote).
#[derive(Debug)]
struct Network;
#[async_trait]
impl DbConnector for Network {
    async fn connect(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<DbIo, DbError> {
        DirectConnector.connect(destination, cancel).await
    }
    async fn connect_path(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<(DbIo, DbPath), DbError> {
        Ok((
            DirectConnector.connect(destination, cancel).await?,
            DbPath::Network,
        ))
    }
}

/// Counts the sockets the session opens through its route.
#[derive(Debug, Default)]
struct Counting(std::sync::atomic::AtomicUsize);
#[async_trait]
impl DbConnector for Counting {
    async fn connect(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<DbIo, DbError> {
        Ok(self.connect_path(destination, cancel).await?.0)
    }
    async fn connect_path(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<(DbIo, DbPath), DbError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        DirectConnector.connect_path(destination, cancel).await
    }
}

/// A TLS front for the plaintext node: a certificate for `localhost` issued
/// by the returned CA, decrypted bytes forwarded to `upstream`.
async fn tls_front(upstream: u16) -> (u16, String) {
    let mut authority = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    authority.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let authority_key = rcgen::KeyPair::generate().unwrap();
    let authority_pem = authority.self_signed(&authority_key).unwrap().pem();
    let issuer = rcgen::Issuer::new(authority, authority_key);
    let key = rcgen::KeyPair::generate().unwrap();
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certificate.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut secure) = acceptor.accept(socket).await else {
                    return;
                };
                let Ok(mut plain) = tokio::net::TcpStream::connect(("127.0.0.1", upstream)).await
                else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut secure, &mut plain).await;
            });
        }
    });
    (port, authority_pem)
}

async fn open(
    connection: &DbConnection,
    credentials: &DbCredentials,
    connector: Arc<dyn DbConnector>,
) -> Result<Arc<DbSession>, DbError> {
    tokio::time::timeout(
        Duration::from_secs(60),
        DbSession::open("live".into(), connection, credentials, connector, false),
    )
    .await
    .expect("open finishes")
    .map(Arc::new)
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

#[derive(Default)]
struct Output {
    columns: Vec<String>,
    primary_key: Vec<String>,
    editable: Option<DbObjectRef>,
    rows: Vec<Vec<DbCell>>,
    pauses: usize,
}

async fn consume(session: &Arc<DbSession>, query: &DbQueryRequest) -> Result<Output, String> {
    let mut stream = session
        .execute(query)
        .map_err(|error| format!("{error:?}"))?;
    let mut output = Output::default();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Started {
                columns,
                editable_table,
                ..
            } => {
                output.primary_key = columns
                    .iter()
                    .filter(|column| column.primary_key)
                    .map(|column| column.name.clone())
                    .collect();
                output.columns = columns.into_iter().map(|column| column.name).collect();
                output.editable = editable_table;
                output.rows.clear();
            }
            DbQueryEvent::Rows { rows, .. } => output.rows.extend(rows),
            DbQueryEvent::Paused { .. } => {
                output.pauses += 1;
                stream.control().load_more(1000).unwrap();
            }
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok(output)
}

async fn run(session: &Arc<DbSession>, text: &str) -> Result<Output, String> {
    consume(session, &request(text)).await
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
    approved.approval_ticket = Some(review.ticket);
    consume(session, &approved).await
}

fn object(kind: DbObjectKind, name: &str) -> DbObjectRef {
    DbObjectRef {
        database: "cc_live".into(),
        schema: None,
        kind,
        name: name.into(),
        parent_name: None,
    }
}

async fn scenario(variable: &str, product: &str) {
    let Some((connection, credentials)) = target(variable, false) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    let dev = open(&connection, &credentials, Arc::new(DirectConnector))
        .await
        .unwrap();
    let info = dev.info().clone();
    assert!(
        info.server_version.starts_with(product),
        "{}",
        info.server_version
    );
    assert_eq!(info.server_read_only, DbServerReadOnly::Unverified);
    assert_eq!(info.transport_security, DbTransportSecurity::Tunnel);
    dev.guard().allow_session_write().unwrap();
    let scylla = product == "ScyllaDB";
    write(&dev, "DROP KEYSPACE IF EXISTS cc_live")
        .await
        .unwrap();
    write(
        &dev,
        &format!(
            "CREATE KEYSPACE cc_live WITH replication = {{'class': 'SimpleStrategy', 'replication_factor': 1}}{}",
            if scylla {
                // Lightweight transactions (cell edits) need vnodes on 6.x.
                " AND tablets = {'enabled': false}"
            } else {
                ""
            }
        ),
    )
    .await
    .unwrap();
    write(&dev, "CREATE TYPE cc_live.address (street text, zip int)")
        .await
        .unwrap();
    write(
        &dev,
        "CREATE TABLE cc_live.events (tenant text, day date, ts timestamp, id uuid, \
         tags set<text>, attrs map<text, int>, scores list<double>, addr frozen<address>, \
         pair tuple<int, text>, payload blob, note text, amount decimal, big varint, \
         dur duration, ip inet, tu timeuuid, PRIMARY KEY ((tenant, day), ts, id)) \
         WITH CLUSTERING ORDER BY (ts DESC, id ASC) AND comment = 'live events'",
    )
    .await
    .unwrap();
    write(&dev, "CREATE INDEX events_note ON cc_live.events (note)")
        .await
        .unwrap();
    write(
        &dev,
        "CREATE TABLE cc_live.bulk (p int, c int, v text, PRIMARY KEY (p, c))",
    )
    .await
    .unwrap();

    // Collections, tuples, user types and scalars decode for the grid.
    write(
        &dev,
        "INSERT INTO cc_live.events (tenant, day, ts, id, tags, attrs, scores, addr, pair, payload, \
         note, amount, big, dur, ip, tu) VALUES ('t1', '2024-01-01', '2024-01-01 10:00:00+0000', \
         123e4567-e89b-12d3-a456-426614174000, {'b', 'a'}, {'x': 1}, [1.5, 2.0], \
         {street: 'Main', zip: 101}, (1, 'one'), 0xcafe, 'first', 12.50, \
         123456789012345678901234567890, 1h30m, '10.0.0.1', \
         50554d6e-29bb-11e5-b345-feff819cdc9f)",
    )
    .await
    .unwrap();
    let row = run(
        &dev,
        "SELECT * FROM cc_live.events WHERE tenant = 't1' AND day = '2024-01-01'",
    )
    .await
    .unwrap();
    assert_eq!(row.rows.len(), 1);
    let value =
        |name: &str| row.rows[0][row.columns.iter().position(|c| c == name).unwrap()].clone();
    assert_eq!(value("tags"), DbCell::Json(r#"["a","b"]"#.into()));
    assert_eq!(value("attrs"), DbCell::Json(r#"{"x":1}"#.into()));
    assert_eq!(value("scores"), DbCell::Json("[1.5,2.0]".into()));
    assert_eq!(
        value("addr"),
        DbCell::Json(r#"{"street":"Main","zip":101}"#.into())
    );
    assert_eq!(value("pair"), DbCell::Json(r#"[1,"one"]"#.into()));
    assert_eq!(value("payload"), DbCell::Binary("yv4=".into()));
    assert_eq!(value("amount"), DbCell::Decimal("12.50".into()));
    assert_eq!(
        value("big"),
        DbCell::Int("123456789012345678901234567890".into())
    );
    assert_eq!(value("dur"), DbCell::Text("1h30m".into()));
    assert_eq!(value("ip"), DbCell::Text("10.0.0.1".into()));
    assert_eq!(value("day"), DbCell::Date("2024-01-01".into()));
    assert_eq!(
        value("ts"),
        DbCell::Timestamp("2024-01-01 10:00:00.000Z".into())
    );
    assert_eq!(
        value("tu"),
        DbCell::Text("50554d6e-29bb-11e5-b345-feff819cdc9f".into())
    );

    // The full primary key makes the result editable; edits use IF EXISTS.
    assert_eq!(row.editable, Some(object(DbObjectKind::Table, "events")));
    assert_eq!(row.primary_key, ["tenant", "day", "ts", "id"]);
    let key: BTreeMap<String, DbCell> = ["tenant", "day", "ts", "id"]
        .iter()
        .map(|name| ((*name).to_owned(), value(name)))
        .collect();
    let edits = vec![
        DbCellEdit {
            original_primary_key: key.clone(),
            column: "note".into(),
            value: DbCell::Text("it's edited".into()),
        },
        DbCellEdit {
            original_primary_key: key.clone(),
            column: "attrs".into(),
            value: DbCell::Json(r#"{"y":2}"#.into()),
        },
    ];
    let table = object(DbObjectKind::Table, "events");
    let edit = dev.review_cell_edits(&table, &edits).await.unwrap();
    assert!(edit.preview.contains("IF EXISTS"), "{}", edit.preview);
    assert!(edit.preview.contains("'it''s edited'"), "{}", edit.preview);
    assert_eq!(dev.apply_cell_edits(&edit.request).await.unwrap(), 1);
    let edited = run(
        &dev,
        "SELECT note, attrs FROM cc_live.events WHERE tenant = 't1' AND day = '2024-01-01'",
    )
    .await
    .unwrap();
    assert_eq!(edited.rows[0][0], DbCell::Text("it's edited".into()));
    assert_eq!(edited.rows[0][1], DbCell::Json(r#"{"y":2}"#.into()));
    // Key columns are not editable; a row that is gone is not recreated.
    let mut wrong = edits[0].clone();
    wrong.column = "ts".into();
    assert!(dev.review_cell_edits(&table, &[wrong]).await.is_err());
    let mut gone = edits[0].clone();
    gone.original_primary_key
        .insert("tenant".into(), DbCell::Text("nobody".into()));
    let edit = dev.review_cell_edits(&table, &[gone]).await.unwrap();
    assert_eq!(
        dev.apply_cell_edits(&edit.request).await,
        Err(DbError::QueryFailed)
    );
    assert!(run(
        &dev,
        "SELECT * FROM cc_live.events WHERE tenant = 'nobody' AND day = '2024-01-01'"
    )
    .await
    .unwrap()
    .rows
    .is_empty());

    // CQL has no EXPLAIN: the measured form traces one run per node.
    assert_eq!(
        dev.explain(&request("SELECT * FROM cc_live.events"), false)
            .await
            .err(),
        Some(DbError::InvalidRequest)
    );
    let traced = dev
        .explain(
            &request("SELECT note FROM cc_live.events WHERE tenant = 't1' AND day = '2024-01-01'"),
            true,
        )
        .await
        .unwrap();
    assert_eq!(traced["analyzed"], true);
    let trace = &traced["plan"]["cql_trace"];
    assert!(trace["duration_us"].as_i64().is_some(), "{traced}");
    let nodes = trace["nodes"].as_array().unwrap();
    assert!(!nodes.is_empty(), "{traced}");
    assert!(
        nodes[0]["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|e| e["activity"].is_string())),
        "{traced}"
    );

    // Rows of one partition change in one conditional batch: all or none.
    write(
        &dev,
        "CREATE TABLE cc_live.batch_edits (p int, c int, v text, PRIMARY KEY (p, c))",
    )
    .await
    .unwrap();
    for (p, c) in [(1, 1), (1, 2), (2, 1)] {
        write(
            &dev,
            &format!("INSERT INTO cc_live.batch_edits (p, c, v) VALUES ({p}, {c}, 'old')"),
        )
        .await
        .unwrap();
    }
    let batch_table = object(DbObjectKind::Table, "batch_edits");
    let cell = |p: i64, c: i64, v: &str| DbCellEdit {
        original_primary_key: BTreeMap::from([
            ("p".to_string(), DbCell::Int(p.to_string())),
            ("c".to_string(), DbCell::Int(c.to_string())),
        ]),
        column: "v".into(),
        value: DbCell::Text(v.into()),
    };
    async fn values(session: &Arc<DbSession>) -> Vec<DbCell> {
        run(session, "SELECT v FROM cc_live.batch_edits")
            .await
            .unwrap()
            .rows
            .into_iter()
            .map(|row| row[0].clone())
            .collect()
    }
    let edit = dev
        .review_cell_edits(
            &batch_table,
            &[cell(1, 1, "new"), cell(1, 2, "new"), cell(2, 1, "new")],
        )
        .await
        .unwrap();
    assert_eq!((edit.rows, edit.partitions), (3, 2));
    assert!(edit.preview.starts_with("BEGIN BATCH"), "{}", edit.preview);
    assert_eq!(dev.apply_cell_edits(&edit.request).await.unwrap(), 3);
    assert!(values(&dev)
        .await
        .iter()
        .all(|v| *v == DbCell::Text("new".into())));
    // One row of the partition is gone: the other is not changed either.
    let edit = dev
        .review_cell_edits(&batch_table, &[cell(1, 1, "lost"), cell(1, 9, "lost")])
        .await
        .unwrap();
    assert_eq!((edit.rows, edit.partitions), (2, 1));
    assert_eq!(
        dev.apply_cell_edits(&edit.request).await,
        Err(DbError::QueryFailed)
    );
    assert!(values(&dev)
        .await
        .iter()
        .all(|v| *v == DbCell::Text("new".into())));

    // Bound parameters follow the server's types.
    let mut parameterized = request(
        "SELECT note FROM cc_live.events WHERE tenant = {{tenant}} AND day = {{day}} AND ts = {{ts}}",
    );
    parameterized.parameters = vec![
        cc_db_core::DbParameter {
            name: "tenant".into(),
            value: DbCell::Text("t1".into()),
            type_hint: None,
        },
        cc_db_core::DbParameter {
            name: "day".into(),
            value: DbCell::Text("2024-01-01".into()),
            type_hint: None,
        },
        cc_db_core::DbParameter {
            name: "ts".into(),
            value: DbCell::Text("2024-01-01T10:00:00Z".into()),
            type_hint: None,
        },
    ];
    assert_eq!(consume(&dev, &parameterized).await.unwrap().rows.len(), 1);

    // More rows than one CQL page and than the first grid page.
    for chunk in (0..ROWS).collect::<Vec<_>>().chunks(100) {
        let inserts: Vec<String> = chunk
            .iter()
            .map(|c| format!("INSERT INTO cc_live.bulk (p, c, v) VALUES (1, {c}, 'v{c}');"))
            .collect();
        write(
            &dev,
            &format!("BEGIN UNLOGGED BATCH\n{}\nAPPLY BATCH", inserts.join("\n")),
        )
        .await
        .unwrap();
    }
    let all = run(&dev, "SELECT * FROM cc_live.bulk WHERE p = 1")
        .await
        .unwrap();
    assert_eq!(all.rows.len(), ROWS);
    assert_eq!(all.pauses, 1, "the first 1000 rows, then one load more");
    assert_eq!(all.rows[ROWS - 1][1], DbCell::Int((ROWS - 1).to_string()));

    // Cancel while the grid waits for "load more".
    let mut stream = dev
        .execute(&request("SELECT * FROM cc_live.bulk WHERE p = 1"))
        .unwrap();
    let mut cancelled = false;
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Paused { .. } => stream.cancel(),
            DbQueryEvent::Cancelled => {
                cancelled = true;
                break;
            }
            DbQueryEvent::Done => break,
            _ => {}
        }
    }
    assert!(cancelled);
    // The session keeps working afterwards.
    assert_eq!(
        run(&dev, "SELECT c FROM cc_live.bulk WHERE p = 1 LIMIT 3")
            .await
            .unwrap()
            .rows
            .len(),
        3
    );

    // USE changes the keyspace of unqualified names.
    run(&dev, "USE cc_live").await.unwrap();
    assert_eq!(
        run(&dev, "SELECT v FROM bulk WHERE p = 1 AND c = 7")
            .await
            .unwrap()
            .rows[0][0],
        DbCell::Text("v7".into())
    );

    // Structure, DDL, details and completion.
    let keyspaces = dev.tree(None, None).await.unwrap().nodes;
    assert!(keyspaces.iter().any(|n| n.reference.name == "cc_live"));
    let objects = dev
        .tree(Some(&object(DbObjectKind::Database, "cc_live")), None)
        .await
        .unwrap()
        .nodes;
    let kinds: Vec<(DbObjectKind, String)> = objects
        .iter()
        .map(|n| (n.reference.kind, n.reference.name.clone()))
        .collect();
    assert!(
        kinds.contains(&(DbObjectKind::Table, "events".into())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(DbObjectKind::UserType, "address".into())),
        "{kinds:?}"
    );
    let columns = dev.tree(Some(&table), None).await.unwrap().nodes;
    let tenant = columns
        .iter()
        .find(|n| n.reference.name == "tenant")
        .unwrap();
    assert!(tenant.primary_key);
    assert_eq!(tenant.detail.as_deref(), Some("text PARTITION KEY"));
    let ts = columns.iter().find(|n| n.reference.name == "ts").unwrap();
    assert_eq!(ts.detail.as_deref(), Some("timestamp CLUSTERING DESC"));
    assert!(columns
        .iter()
        .any(|n| n.reference.kind == DbObjectKind::Index && n.reference.name == "events_note"));
    let fields = dev
        .tree(Some(&object(DbObjectKind::UserType, "address")), None)
        .await
        .unwrap()
        .nodes;
    assert_eq!(fields.len(), 2);
    let ddl = dev.ddl(&table).await.unwrap();
    assert!(ddl.contains("CREATE TABLE cc_live.events"), "{ddl}");
    assert!(ddl.contains("PRIMARY KEY ((tenant, day), ts, id)"), "{ddl}");
    assert!(
        ddl.contains("CLUSTERING ORDER BY (ts DESC, id ASC)"),
        "{ddl}"
    );
    // The fallback without server DESCRIBE builds the same definition.
    let built = dev.cassandra_schema_ddl(&table).await.unwrap();
    assert!(
        built.contains("PRIMARY KEY ((tenant, day), ts, id)"),
        "{built}"
    );
    assert!(
        built.contains("CLUSTERING ORDER BY (ts DESC, id ASC)"),
        "{built}"
    );
    assert!(built.contains("AND comment = 'live events'"), "{built}");
    assert!(
        built.contains("CREATE INDEX events_note ON cc_live.events (note);"),
        "{built}"
    );
    for kind in [DbObjectKind::Database, DbObjectKind::UserType] {
        let name = if kind == DbObjectKind::Database {
            "cc_live"
        } else {
            "address"
        };
        dev.cassandra_schema_ddl(&object(kind, name)).await.unwrap();
    }
    let type_ddl = dev
        .ddl(&object(DbObjectKind::UserType, "address"))
        .await
        .unwrap();
    assert!(
        type_ddl.contains("CREATE TYPE cc_live.address"),
        "{type_ddl}"
    );
    let keyspace_ddl = dev
        .ddl(&object(DbObjectKind::Database, "cc_live"))
        .await
        .unwrap();
    assert!(
        keyspace_ddl.contains("CREATE KEYSPACE cc_live"),
        "{keyspace_ddl}"
    );
    let details = serde_json::to_value(dev.object_details(&table).await.unwrap()).unwrap();
    let text = details.to_string();
    assert!(text.contains("\"gc_grace_seconds\""), "{text}");
    assert!(text.contains("tenant, day"), "{text}");
    let keyspace_details = serde_json::to_value(
        dev.object_details(&object(DbObjectKind::Database, "cc_live"))
            .await
            .unwrap(),
    )
    .unwrap()
    .to_string();
    assert!(
        keyspace_details.contains("SimpleStrategy"),
        "{keyspace_details}"
    );
    let completions = dev.complete("SELECT * FROM ev", 16).await.unwrap();
    assert!(completions.iter().any(|c| c.label == "events"));
    assert_eq!(
        dev.object_query(&table, 50).await.unwrap(),
        "SELECT * FROM cc_live.events LIMIT 50;"
    );

    // Every driver socket goes through the route: the probe, the control
    // connection and the one pool connection of the single coordinator,
    // although the node announces its own (reachable) container address.
    let counting = Arc::new(Counting::default());
    let routed = open(&connection, &credentials, counting.clone())
        .await
        .unwrap();
    run(&routed, "SELECT now() FROM system.local")
        .await
        .unwrap();
    assert_eq!(counting.0.load(std::sync::atomic::Ordering::SeqCst), 3);
    routed.close();

    // Production: reads only; writes and role management are refused.
    let (production, credentials) = target(variable, true).unwrap();
    let prod = open(&production, &credentials, Arc::new(DirectConnector))
        .await
        .unwrap();
    assert!(prod.info().read_only);
    assert_eq!(
        run(&prod, "SELECT v FROM cc_live.bulk WHERE p = 1 AND c = 1")
            .await
            .unwrap()
            .rows
            .len(),
        1
    );
    for refused in [
        "INSERT INTO cc_live.bulk (p, c, v) VALUES (2, 1, 'x')",
        "TRUNCATE cc_live.bulk",
        "GRANT SELECT ON KEYSPACE cc_live TO cassandra",
    ] {
        assert!(
            matches!(prod.execute(&request(refused)), Err(DbError::PolicyDenied)),
            "{refused}"
        );
    }
    assert!(prod.review_cell_edits(&table, &edits).await.is_err());
    prod.close();

    // TLS: no TLS on this port. Over the network "prefer" never continues
    // without it; "verify" fails even on this computer.
    let mut tls = connection.clone();
    tls.tls.mode = DbTlsMode::Prefer;
    assert_eq!(
        open(&tls, &credentials, Arc::new(Network)).await.err(),
        Some(DbError::TlsUnavailable)
    );
    tls.tls.mode = DbTlsMode::Verify;
    tls.host = "localhost".into();
    assert_eq!(
        open(&tls, &credentials, Arc::new(DirectConnector))
            .await
            .err(),
        Some(DbError::TlsUnavailable)
    );
    // "prefer" on this computer continues without TLS and says so.
    tls.tls.mode = DbTlsMode::Prefer;
    let local = open(&tls, &credentials, Arc::new(DirectConnector))
        .await
        .unwrap();
    assert_eq!(local.info().transport_security, DbTransportSecurity::Tunnel);
    local.close();
    if credentials.password.is_some() {
        // A password never travels in plaintext over the network.
        assert_eq!(
            open(&connection, &credentials, Arc::new(Network))
                .await
                .err(),
            Some(DbError::CleartextRefused)
        );
        let wrong = DbCredentials {
            password: Some(SecretString::from("wrong-password")),
            client_key_pem: None,
        };
        assert_eq!(
            open(&connection, &wrong, Arc::new(DirectConnector))
                .await
                .err(),
            Some(DbError::AuthenticationFailed)
        );
    }
    // A verified TLS session end to end (through a TLS front), even over the
    // network; a certificate from another CA is refused.
    let (port, authority) = tls_front(connection.port).await;
    let mut secure = connection.clone();
    secure.host = "localhost".into();
    secure.port = port;
    secure.tls.mode = DbTlsMode::Verify;
    secure.tls.certificate_authority_pem = Some(authority);
    let verified = open(&secure, &credentials, Arc::new(Network))
        .await
        .unwrap();
    assert_eq!(
        verified.info().transport_security,
        DbTransportSecurity::Verified
    );
    assert_eq!(
        run(
            &verified,
            "SELECT v FROM cc_live.bulk WHERE p = 1 AND c = 2"
        )
        .await
        .unwrap()
        .rows[0][0],
        DbCell::Text("v2".into())
    );
    verified.close();
    let (_, other) = tls_front(connection.port).await;
    secure.tls.certificate_authority_pem = Some(other);
    assert_eq!(
        open(&secure, &credentials, Arc::new(Network)).await.err(),
        Some(DbError::TlsFailed)
    );
    secure.tls.certificate_authority_pem = None;
    secure.tls.mode = DbTlsMode::Require;
    let unverified = open(&secure, &credentials, Arc::new(Network))
        .await
        .unwrap();
    assert_eq!(
        unverified.info().transport_security,
        DbTransportSecurity::Unverified
    );
    unverified.close();

    let mut elsewhere = connection.clone();
    if let DbOptions::Cassandra {
        local_datacenter, ..
    } = &mut elsewhere.options
    {
        *local_datacenter = Some("no-such-dc".into());
    }
    assert_eq!(
        open(&elsewhere, &credentials, Arc::new(DirectConnector))
            .await
            .err(),
        Some(DbError::DatacenterMismatch)
    );

    write(&dev, "DROP KEYSPACE cc_live").await.unwrap();
    dev.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn scylla_end_to_end() {
    scenario("CC_DB_LIVE_SCYLLA", "ScyllaDB").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cassandra_end_to_end() {
    scenario("CC_DB_LIVE_CASSANDRA", "Apache Cassandra").await;
}
