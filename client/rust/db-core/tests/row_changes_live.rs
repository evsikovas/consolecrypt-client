//! Opt-in acceptance of result row changes (add, change, delete) against
//! disposable local databases: CC_DB_LIVE_PG and CC_DB_LIVE_MYSQL, either
//! `scheme://user:password@host:port/database` or
//! `host:port:user:password:database`. Every run uses its own tables and
//! drops them.
use cc_db_core::{
    row_changes::{DbEditColumn, DbRowChange},
    transport::{DbCredentials, DirectConnector},
    DbCell, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use secrecy::SecretString;
use std::{collections::BTreeMap, sync::Arc};

fn target(variable: &str, engine: DbEngine) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(variable).ok()?;
    let (host, port, user, password, database) = match spec.split_once("://") {
        Some((_, rest)) => {
            let (auth, location) = rest.rsplit_once('@')?;
            let (user, password) = auth.split_once(':')?;
            let (address, database) = location.split_once('/')?;
            let (host, port) = address.rsplit_once(':')?;
            (
                host.to_owned(),
                port.to_owned(),
                user.to_owned(),
                password.to_owned(),
                database.to_owned(),
            )
        }
        None => {
            let parts: Vec<&str> = spec.splitn(5, ':').collect();
            (
                parts[0].into(),
                parts[1].into(),
                parts[2].into(),
                parts[3].into(),
                parts[4].into(),
            )
        }
    };
    let mut connection = DbConnection::new("live-rows", &host, engine);
    connection.port = port.parse().ok()?;
    connection.username = user;
    connection.database = database;
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
    Some((
        connection,
        DbCredentials {
            password: Some(SecretString::from(password)),
            client_key_pem: None,
        },
    ))
}

fn request(text: &str) -> DbQueryRequest {
    DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    }
}

fn text(cell: &DbCell) -> String {
    match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Bool(value) => value.to_string(),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => v.clone(),
    }
}

/// Rows of a query plus the editable table its first result reports.
async fn read(
    session: &Arc<DbSession>,
    sql: &str,
    approve: bool,
) -> Result<(Vec<Vec<String>>, Option<DbObjectRef>, Vec<String>), String> {
    let mut query = request(sql);
    if approve {
        let review = session
            .guard()
            .review(&query)
            .map_err(|e| format!("{e:?}"))?;
        session
            .guard()
            .approve(&review.ticket)
            .map_err(|e| format!("{e:?}"))?;
        query.approval_ticket = Some(review.ticket);
    }
    let mut stream = session.execute(&query).map_err(|e| format!("{e:?}"))?;
    let (mut rows, mut table, mut keys) = (Vec::new(), None, Vec::new());
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Started {
                columns,
                editable_table,
                result_index: 0,
            } => {
                table = editable_table;
                keys = columns
                    .iter()
                    .filter(|c| c.primary_key)
                    .map(|c| c.name.clone())
                    .collect();
            }
            DbQueryEvent::Rows { rows: batch, .. } => {
                rows.extend(batch.iter().map(|row| row.iter().map(text).collect()))
            }
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok((rows, table, keys))
}

async fn write(session: &Arc<DbSession>, sql: &str) {
    read(session, sql, true).await.expect(sql);
}

/// Review → approve → apply, as the app does after the confirmation.
async fn save(
    session: &Arc<DbSession>,
    table: &DbObjectRef,
    changes: Vec<DbRowChange>,
) -> Result<u64, String> {
    let generated = session
        .review_row_changes(table, &changes)
        .await
        .map_err(|e| format!("review {e:?}"))?;
    let mut approved = generated.request.clone();
    let review = session
        .guard()
        .review(&approved)
        .map_err(|e| format!("{e:?}"))?;
    session
        .guard()
        .approve(&review.ticket)
        .map_err(|e| format!("{e:?}"))?;
    approved.approval_ticket = Some(review.ticket);
    session
        .apply_cell_edits(&approved)
        .await
        .map_err(|e| format!("apply {e:?}"))
}

fn values(pairs: &[(&str, DbCell)]) -> BTreeMap<String, DbCell> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect()
}

fn column<'a>(columns: &'a [DbEditColumn], name: &str) -> &'a DbEditColumn {
    columns.iter().find(|c| c.name == name).expect(name)
}

async fn exercise(variable: &str, engine: DbEngine) {
    let Some((connection, credentials)) = target(variable, engine) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    let session = Arc::new(
        DbSession::open(
            "rows".into(),
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            false,
        )
        .await
        .unwrap(),
    );
    let suffix = format!("{:08x}", rand_suffix());
    let items = format!("cc_rows_{suffix}");
    let coded = format!("cc_rows_uk_{suffix}");
    let (create_items, create_coded) = match engine {
        DbEngine::Postgresql => (
            format!(
                "CREATE TABLE {items} (id serial PRIMARY KEY, name varchar(40) NOT NULL, \
                 note varchar(40), created timestamptz NOT NULL DEFAULT now(), \
                 doubled int GENERATED ALWAYS AS (char_length(name) * 2) STORED)"
            ),
            format!("CREATE TABLE {coded} (code varchar(10) NOT NULL UNIQUE, amount int, label text UNIQUE)"),
        ),
        _ => (
            format!(
                "CREATE TABLE {items} (id int AUTO_INCREMENT PRIMARY KEY, name varchar(40) NOT NULL, \
                 note varchar(40), created timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                 doubled int GENERATED ALWAYS AS (char_length(name) * 2) STORED)"
            ),
            format!("CREATE TABLE {coded} (code varchar(10) NOT NULL UNIQUE, amount int, label varchar(20) UNIQUE)"),
        ),
    };
    write(&session, &create_items).await;
    write(&session, &create_coded).await;
    let result = async {
        // A single-table read is editable by its primary key.
        let (_, table, keys) = read(&session, &format!("SELECT * FROM {items}"), false)
            .await
            .unwrap();
        let table = table.expect("editable table");
        assert_eq!(keys, ["id"]);
        let columns = session.edit_columns(&table).await.unwrap();
        assert!(column(&columns, "id").key && column(&columns, "id").has_default);
        assert!(column(&columns, "name").required());
        assert!(!column(&columns, "note").required());
        assert!(column(&columns, "created").has_default && !column(&columns, "created").required());
        assert!(column(&columns, "doubled").generated);

        // Two new rows: defaults fill id and created, NULL stays NULL.
        assert_eq!(
            save(
                &session,
                &table,
                vec![
                    DbRowChange::Insert {
                        values: values(&[("name", DbCell::Text("alpha".into()))]),
                    },
                    DbRowChange::Insert {
                        values: values(&[
                            ("name", DbCell::Text("it's \"beta\"; DROP".into())),
                            ("note", DbCell::Null),
                        ]),
                    },
                ],
            )
            .await
            .unwrap(),
            2
        );
        let order = format!("SELECT id, name, note, doubled FROM {items} ORDER BY id");
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][1], "it's \"beta\"; DROP");
        assert_eq!(rows[1][2], "NULL");
        assert_eq!(rows[0][3], "10", "generated by the server");
        let (first, second) = (rows[0][0].clone(), rows[1][0].clone());
        let key = |id: &str| BTreeMap::from([("id".to_string(), DbCell::Int(id.into()))]);

        // A missing required value and a generated column are refused
        // before anything runs.
        assert!(save(
            &session,
            &table,
            vec![DbRowChange::Insert {
                values: values(&[("note", DbCell::Text("x".into()))])
            }]
        )
        .await
        .is_err());
        assert!(save(
            &session,
            &table,
            vec![DbRowChange::Update {
                original_primary_key: key(&first),
                values: values(&[("doubled", DbCell::Int("1".into()))]),
            }]
        )
        .await
        .is_err());

        // Change, delete and add in one save.
        assert_eq!(
            save(
                &session,
                &table,
                vec![
                    DbRowChange::Update {
                        original_primary_key: key(&first),
                        values: values(&[("note", DbCell::Text("changed".into()))]),
                    },
                    DbRowChange::Delete {
                        original_primary_key: key(&second),
                    },
                    DbRowChange::Insert {
                        values: values(&[("name", DbCell::Text("gamma".into()))]),
                    },
                ],
            )
            .await
            .unwrap(),
            3
        );
        let (rows, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0][1].as_str(), rows[0][2].as_str()),
            ("alpha", "changed")
        );
        assert_eq!(rows[1][1], "gamma");

        // A row that is gone rolls the whole save back.
        assert!(save(
            &session,
            &table,
            vec![
                DbRowChange::Insert {
                    values: values(&[("name", DbCell::Text("ghost".into()))]),
                },
                DbRowChange::Delete {
                    original_primary_key: key("999999"),
                },
            ],
        )
        .await
        .is_err());
        let (after, ..) = read(&session, &order, false).await.unwrap();
        assert_eq!(after, rows);

        // Without a primary key the first unique key of NOT NULL columns
        // identifies rows (the nullable unique key is not used).
        write(
            &session,
            &format!("INSERT INTO {coded} (code, amount) VALUES ('a', 1), ('b', 2)"),
        )
        .await;
        let (_, table, keys) = read(&session, &format!("SELECT * FROM {coded}"), false)
            .await
            .unwrap();
        let table = table.expect("editable by its unique key");
        assert_eq!(keys, ["code"]);
        let code = |value: &str| BTreeMap::from([("code".to_string(), DbCell::Text(value.into()))]);
        assert_eq!(
            save(
                &session,
                &table,
                vec![
                    DbRowChange::Update {
                        original_primary_key: code("a"),
                        values: values(&[("amount", DbCell::Int("10".into()))]),
                    },
                    DbRowChange::Delete {
                        original_primary_key: code("b"),
                    },
                ],
            )
            .await
            .unwrap(),
            2
        );
        let (rows, ..) = read(
            &session,
            &format!("SELECT code, amount FROM {coded} ORDER BY code"),
            false,
        )
        .await
        .unwrap();
        assert_eq!(rows, vec![vec!["a".to_string(), "10".to_string()]]);
        assert_eq!(table.kind, DbObjectKind::Table);
    };
    // Drop the tables even when an assertion fails.
    let outcome = futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(result)).await;
    write(&session, &format!("DROP TABLE {items}")).await;
    write(&session, &format!("DROP TABLE {coded}")).await;
    session.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

fn rand_suffix() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    nanos ^ std::process::id().rotate_left(16)
}

#[tokio::test]
async fn postgres_row_changes() {
    exercise("CC_DB_LIVE_PG", DbEngine::Postgresql).await;
}

#[tokio::test]
async fn mysql_row_changes() {
    exercise("CC_DB_LIVE_MYSQL", DbEngine::Mysql).await;
}
