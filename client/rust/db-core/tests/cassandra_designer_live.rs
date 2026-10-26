//! Opt-in table designer on ScyllaDB / Cassandra (FEAT-08):
//! CC_DB_LIVE_SCYLLA / CC_DB_LIVE_CASSANDRA = host:port[:user:password], as
//! in `cassandra_live.rs`. Creates, loads and alters a table in the keyspace
//! `cc_design_live`, dropped at the end.
use cc_db_core::{
    designer::{self, ColumnDesign, DesignPlan, IndexDesign, KeyDesign, TableDesign},
    transport::{DbCredentials, DirectConnector},
    DbCell, DbObjectKind, DbObjectRef, DbQueryEvent, DbQueryRequest, DbSession,
};
use cc_models::database::{CqlConsistency, DbConnection, DbEngine, DbOptions, DbTlsMode};
use secrecy::SecretString;
use std::{sync::Arc, time::Duration};

fn target(variable: &str) -> Option<(DbConnection, DbCredentials)> {
    let spec = std::env::var(variable).ok()?;
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let mut connection = DbConnection::new("live", parts[0], DbEngine::Cassandra);
    connection.port = parts[1].parse().unwrap();
    connection.username = parts.get(2).copied().unwrap_or_default().into();
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
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
            password: parts.get(3).map(|p| SecretString::from((*p).to_owned())),
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

async fn consume(
    session: &Arc<DbSession>,
    query: &DbQueryRequest,
) -> Result<Vec<Vec<DbCell>>, String> {
    let mut stream = session.execute(query).map_err(|e| format!("{e:?}"))?;
    let mut rows = Vec::new();
    while let Some(event) = stream.events.recv().await {
        match event {
            DbQueryEvent::Rows { rows: batch, .. } => rows.extend(batch),
            DbQueryEvent::Paused { .. } => stream.control().load_more(1000).unwrap(),
            DbQueryEvent::Error { code, .. } => return Err(code),
            DbQueryEvent::Done | DbQueryEvent::Cancelled => break,
            _ => {}
        }
    }
    Ok(rows)
}

async fn write(session: &Arc<DbSession>, text: &str) {
    let review = session.guard().review(&request(text)).expect(text);
    session.guard().approve(&review.ticket).unwrap();
    let mut approved = request(text);
    approved.approval_ticket = Some(review.ticket);
    consume(session, &approved).await.expect(text);
}

async fn apply(session: &Arc<DbSession>, plan: &DesignPlan) {
    assert!(!plan.has_errors(), "{:?}", plan.issues);
    let mut request = request(&plan.script);
    let review = session.guard().review(&request).expect("review");
    session.guard().approve(&review.ticket).unwrap();
    request.approval_ticket = Some(review.ticket);
    let result = session.apply_design(&request).await.unwrap();
    assert!(result.committed, "{result:?}");
}

fn column(name: &str, data_type: &str) -> ColumnDesign {
    ColumnDesign {
        name: name.into(),
        data_type: data_type.into(),
        nullable: true,
        ..Default::default()
    }
}

async fn scenario(variable: &str) {
    let Some((connection, credentials)) = target(variable) else {
        eprintln!("{variable} not set; skipped");
        return;
    };
    let session = Arc::new(
        tokio::time::timeout(
            Duration::from_secs(60),
            DbSession::open(
                "live".into(),
                &connection,
                &credentials,
                Arc::new(DirectConnector),
                false,
            ),
        )
        .await
        .expect("open finishes")
        .expect("open"),
    );
    write(
        &session,
        "CREATE KEYSPACE IF NOT EXISTS cc_design_live WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}",
    )
    .await;
    let design = TableDesign {
        database: "cc_design_live".into(),
        name: "readings".into(),
        comment: "sensor's data".into(),
        columns: vec![
            column("sensor", "uuid"),
            column("day", "date"),
            column("at", "timestamp"),
            column("value", "double"),
            column("Tags", "set<text>"),
        ],
        primary_key: KeyDesign {
            columns: vec!["sensor".into(), "day".into(), "at".into()],
            ..Default::default()
        },
        indexes: vec![IndexDesign {
            columns: vec!["value".into()],
            ..Default::default()
        }],
        options: [
            ("partition_key_columns".to_owned(), "2".to_owned()),
            ("clustering_order".to_owned(), "at DESC".to_owned()),
            ("default_time_to_live".to_owned(), "86400".to_owned()),
        ]
        .into(),
        ..Default::default()
    };
    apply(
        &session,
        &designer::plan(DbEngine::Cassandra, None, &design),
    )
    .await;

    let table = DbObjectRef {
        database: "cc_design_live".into(),
        schema: None,
        kind: DbObjectKind::Table,
        name: "readings".into(),
        parent_name: None,
    };
    let loaded = session.table_design(&table).await.unwrap();
    assert_eq!(loaded.primary_key.columns, ["sensor", "day", "at"]);
    assert_eq!(loaded.options["partition_key_columns"], "2");
    assert_eq!(loaded.options["clustering_order"], "at DESC");
    assert_eq!(loaded.options["default_time_to_live"], "86400");
    assert_eq!(loaded.comment, "sensor's data");
    assert_eq!(loaded.indexes.len(), 1, "{:?}", loaded.indexes);
    assert_eq!(loaded.indexes[0].columns, ["value"]);
    let unchanged = designer::plan(DbEngine::Cassandra, Some(&loaded), &loaded);
    assert!(
        unchanged.statements.is_empty() && !unchanged.has_errors(),
        "{unchanged:?}"
    );

    // Drop and add columns, rename a clustering column, change options,
    // drop the index.
    let mut after = loaded.clone();
    after.columns.retain(|c| c.name != "Tags");
    let at = after.columns.iter_mut().find(|c| c.name == "at").unwrap();
    at.name = "ts".into();
    for key in &mut after.primary_key.columns {
        if key == "at" {
            *key = "ts".into();
        }
    }
    after.columns.push(column("unit", "text"));
    after.indexes.clear();
    after.comment = "readings".into();
    after
        .options
        .insert("gc_grace_seconds".into(), "3600".into());
    apply(
        &session,
        &designer::plan(DbEngine::Cassandra, Some(&loaded), &after),
    )
    .await;
    let reloaded = session.table_design(&table).await.unwrap();
    let names: Vec<&str> = reloaded.columns.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"ts") && names.contains(&"unit") && !names.contains(&"Tags"),
        "{names:?}"
    );
    assert_eq!(reloaded.options["clustering_order"], "ts DESC");
    assert_eq!(reloaded.options["gc_grace_seconds"], "3600");
    assert_eq!(reloaded.comment, "readings");
    assert!(reloaded.indexes.is_empty());

    write(&session, "DROP KEYSPACE cc_design_live").await;
}

#[tokio::test]
async fn designer_on_scylla() {
    scenario("CC_DB_LIVE_SCYLLA").await;
}

#[tokio::test]
async fn designer_on_cassandra() {
    scenario("CC_DB_LIVE_CASSANDRA").await;
}
