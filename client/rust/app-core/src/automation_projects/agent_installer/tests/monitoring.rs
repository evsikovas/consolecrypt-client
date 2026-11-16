use super::*;

#[tokio::test]
async fn actual_monitoring_factory_binds_group_to_only_selected_host_and_never_executes() {
    let (_dir, app, frame) = fixture().await;
    let context: Value = serde_json::from_str(
        &app.automation_agent_installer_context_json(&frame.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let host = &context["hosts"][0];
    let mut request = frame.clone();
    request.as_object_mut().unwrap().extend(
        json!({
            "name":"Monitoring source", "operation":"install",
            "hostId":host["hostId"], "expectedHostRevision":host["hostRevision"],
            "plan":cc_automation_core::monitoring_node_plan::MonitoringNodePlan::default(),
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    // Invalid sizing is refused before any project is allocated or published.
    request["plan"]["memory_mib"] = json!(1536);
    assert!(
        app.automation_monitoring_node_create_json(&request.to_string())
            .await
            .is_err()
    );
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
    request["plan"]["memory_mib"] = json!(2048);
    for operation in ["install", "restore"] {
        request["operation"] = json!(operation);
        let result: Value = serde_json::from_str(
            &app.automation_monitoring_node_create_json(&request.to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["claimedRelease"], Value::Null);
        assert_eq!(result["actualRuntimeVersion"], Value::Null);
        let id = result["project"]["id"].as_str().unwrap().to_owned();
        let source = app.automation_project_context(&id).await.unwrap();
        let selected = Uuid::parse_str(host["hostId"].as_str().unwrap()).unwrap();
        assert_eq!(source.inventory.selected_hosts, Some(vec![selected]));
        assert_eq!(source.inventory.groups.len(), 1);
        assert_eq!(source.inventory.groups["monitoring_nodes"], vec![selected]);
        let file = app
            .automation_project_file(id, "playbook.yml".into())
            .await
            .unwrap();
        assert!(file.text.contains("hosts: monitoring_nodes"));
        assert!(
            file.text
                .contains(&format!("cc_monitoring_operation: {operation}"))
        );
        assert!(
            file.text
                .contains("ansible_memtotal_mb | int >= cc_monitoring_plan.memory_mib")
        );
        assert!(
            app.automation_jobs().await.unwrap()["jobs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(app.automation_projects().await.unwrap().projects.len(), 2);
}
