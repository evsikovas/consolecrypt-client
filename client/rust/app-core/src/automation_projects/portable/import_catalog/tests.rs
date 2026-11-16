//! Pure metadata FSM tests. These are NOT crypto-witness or SQL-CAS evidence.
use super::*;
struct Model {
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    row: Value,
}
impl Model {
    fn new() -> Self {
        let project = Uuid::new_v4();
        let operation = Uuid::new_v4();
        let transaction = Uuid::new_v4();
        let summary = PortablePackageSummary {
            source_digest: "a".repeat(64),
            portable_digest: "b".repeat(64),
            files: 1,
            bytes: 23,
            exclusions: 2,
        };
        let row = reservation(
            &std::env::temp_dir().join("owned-portable-model-no-files"),
            project,
            operation,
            transaction,
            "Explicit new project",
            &"c".repeat(64),
            &summary,
            1,
        )
        .unwrap();
        Self {
            project,
            operation,
            transaction,
            row,
        }
    }
    fn prepared(&self) -> Value {
        prepared(
            &self.row,
            &managed::row_stamp(&self.row).unwrap(),
            self.project,
            self.operation,
            self.transaction,
            &"d".repeat(64),
            2,
        )
        .unwrap()
    }
}
#[test]
fn reservation_and_prepared_are_visible_metadata_but_deny_all_ordinary_project_consumers() {
    let m = Model::new();
    assert!(m.row.get(FIELD).is_some());
    assert_eq!(m.row["name"], "Explicit new project");
    for row in [&m.row, &m.prepared()] {
        assert!(!ordinary_supported(row));
        assert!(
            !super::super::super::supported(row),
            "shared gate covers ordinary file/inventory/snapshot/catalog writers"
        );
    }
    assert_eq!(m.row["revision"], 1);
    assert_eq!(m.prepared()["revision"], 2);
    assert_eq!(m.row["source"]["kind"], "managed");
    assert!(m.row.get("inventory").is_none() && m.row.get("runner").is_none());
}
#[test]
fn exact_three_step_metadata_keeps_name_identity_and_fingerprint_without_implicit_retry() {
    let m = Model::new();
    let next = m.prepared();
    let stamp = managed::row_stamp(&next).unwrap();
    let selected = ready(
        &next,
        &stamp,
        m.project,
        m.operation,
        m.transaction,
        &"d".repeat(64),
        3,
    )
    .unwrap();
    assert_eq!(selected["revision"], 3);
    assert_eq!(selected["source"]["status"], "ready");
    assert_eq!(selected["name"], m.row["name"]);
    assert!(ordinary_supported(&selected));
    assert_eq!(selected[FIELD]["project"], m.project.to_string());
    assert_eq!(
        selected[FIELD]["reservationRowSha256"],
        managed::row_stamp(&m.row).unwrap()
    );
    assert!(ready(
        &selected,
        &managed::row_stamp(&selected).unwrap(),
        m.project,
        m.operation,
        m.transaction,
        &"d".repeat(64),
        4
    )
    .is_err());
    assert!(prepared(
        &next,
        &stamp,
        m.project,
        m.operation,
        m.transaction,
        &"d".repeat(64),
        4
    )
    .is_err());
}
#[test]
fn future_unknown_or_changed_pending_fields_preserve_exact_raw_value_and_deny_transition() {
    let m = Model::new();
    for mode in [
        "futureVersion",
        "futureState",
        "futureField",
        "futureTop",
        "futureSource",
        "changedIdentity",
    ] {
        let mut row = m.row.clone();
        match mode {
            "futureVersion" => row[FIELD]["version"] = Value::from(2),
            "futureState" => row[FIELD]["state"] = Value::from("future"),
            "futureField" => row[FIELD]["future"] = json!({"keep":"exact raw"}),
            "futureTop" => row["future"] = json!({"keep":"exact raw"}),
            "futureSource" => row["source"]["future"] = Value::from("keep raw"),
            _ => row[FIELD]["transaction"] = Value::from(Uuid::new_v4().to_string()),
        }
        let before = serde_json::to_vec(&row).unwrap();
        assert!(prepared(
            &row,
            &managed::row_stamp(&row).unwrap(),
            m.project,
            m.operation,
            m.transaction,
            &"d".repeat(64),
            2
        )
        .is_err());
        assert_eq!(serde_json::to_vec(&row).unwrap(), before);
        assert!(!ordinary_supported(&row));
    }
}
#[test]
fn stale_row_stamp_wrong_scope_or_fingerprint_cannot_advance_or_overwrite() {
    let m = Model::new();
    let next = m.prepared();
    let before = next.clone();
    let stamp = managed::row_stamp(&next).unwrap();
    assert!(ready(
        &next,
        &"e".repeat(64),
        m.project,
        m.operation,
        m.transaction,
        &"d".repeat(64),
        3
    )
    .is_err());
    assert!(ready(
        &next,
        &stamp,
        Uuid::new_v4(),
        m.operation,
        m.transaction,
        &"d".repeat(64),
        3
    )
    .is_err());
    assert!(ready(
        &next,
        &stamp,
        m.project,
        Uuid::new_v4(),
        m.transaction,
        &"d".repeat(64),
        3
    )
    .is_err());
    assert!(ready(
        &next,
        &stamp,
        m.project,
        m.operation,
        m.transaction,
        &"e".repeat(64),
        3
    )
    .is_err());
    assert_eq!(next, before);
}
#[test]
fn legacy_marker_absence_stays_unchanged_and_conflicting_purposes_are_readonly() {
    assert!(ordinary_supported(
        &json!({"version":1,"source":{"kind":"managed"},"revision":7})
    ));
    let m = Model::new();
    let next = m.prepared();
    let mut selected = ready(
        &next,
        &managed::row_stamp(&next).unwrap(),
        m.project,
        m.operation,
        m.transaction,
        &"d".repeat(64),
        3,
    )
    .unwrap();
    selected["publication"] = json!({"version":1});
    let before = selected.clone();
    assert!(!ordinary_supported(&selected));
    assert_eq!(selected, before);
}
