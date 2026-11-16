use super::*;
use serde_json::json;

const HOST: &str = "5e7c5182-19df-48e1-99ed-87d28c9356e9";
fn labels(entries: &[(&str, &str)]) -> Labels {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}
fn binding(selector: Labels) -> HostBinding {
    HostBinding::new(HOST, "source-1", 1, selector).unwrap()
}
fn wire() -> serde_json::Value {
    json!({"version":1,"hostId":HOST,"sourceId":"source-1","revision":1,"selector":{"instance":"node:9100"}})
}

#[test]
fn canonical_camel_case_binding_round_trip_preserves_native_identity() {
    let raw = serde_json::to_vec(&wire()).unwrap();
    let value = HostBinding::from_json(&raw).unwrap();
    assert_eq!(value.host_id().hyphenated().to_string(), HOST);
    assert_eq!(value.source_id(), "source-1");
    assert_eq!(value.revision(), 1);
    assert_eq!(serde_json::to_value(value).unwrap(), wire());
}

#[test]
fn unknown_future_duplicate_malformed_and_nonpositive_revision_are_rejected() {
    for (key, value) in [
        ("version", json!(2)),
        ("version", json!(true)),
        ("revision", json!(0)),
        ("revision", json!(-1)),
        ("revision", json!(1.0)),
        ("extra", json!("ignored?")),
        ("selector", json!([])),
        ("sourceId", json!(42)),
    ] {
        let mut raw = wire();
        raw[key] = value;
        assert!(
            HostBinding::from_json(&serde_json::to_vec(&raw).unwrap()).is_err(),
            "{key}"
        );
    }
    let duplicate = format!(
        r#"{{"version":1,"hostId":"{HOST}","sourceId":"source-1","revision":1,"selector":{{"instance":"a","instance":"b"}}}}"#
    );
    assert!(HostBinding::from_json(duplicate.as_bytes()).is_err());
    let duplicate_field = format!(
        r#"{{"version":1,"hostId":"{HOST}","sourceId":"source-1","sourceId":"source-2","revision":1,"selector":{{"instance":"a"}}}}"#
    );
    assert!(HostBinding::from_json(duplicate_field.as_bytes()).is_err());
    let mut missing = wire();
    missing.as_object_mut().unwrap().remove("selector");
    assert!(HostBinding::from_json(&serde_json::to_vec(&missing).unwrap()).is_err());
}

#[test]
fn host_id_requires_non_nil_canonical_native_uuid() {
    for id in [
        "host-1".to_owned(),
        "127.0.0.1".to_owned(),
        "node.example.test".to_owned(),
        "00000000-0000-0000-0000-000000000000".to_owned(),
        HOST.to_uppercase(),
        HOST.replace('-', ""),
        format!("{{{HOST}}}"),
        format!(" {HOST}"),
    ] {
        assert_eq!(
            HostBinding::new(&id, "source-1", 1, labels(&[("instance", "node")])),
            Err(MappingError::InvalidHostId)
        );
    }
}

#[test]
fn source_ids_are_opaque_bounded_and_exact_without_uuid_or_url_coercion() {
    for source in [
        "source-1".to_owned(),
        "tenant/source:α".to_owned(),
        "x".repeat(256),
    ] {
        let value = HostBinding::new(HOST, &source, 1, labels(&[("instance", "a")])).unwrap();
        assert_eq!(resolve(&value, &source, &[]).unwrap(), Resolution::Missing);
        assert_eq!(value.source_id(), source);
    }
    for source in ["".to_owned(), "x".repeat(257), "я".repeat(129)] {
        assert_eq!(
            HostBinding::new(HOST, &source, 1, labels(&[("instance", "a")])),
            Err(MappingError::InvalidSourceId)
        );
    }
    let value = binding(labels(&[("instance", "node")]));
    for mismatch in ["source-01", "SOURCE-1", "source-1/", "other-source"] {
        assert_eq!(
            resolve(&value, mismatch, &[labels(&[("instance", "node")])]),
            Err(MappingError::SourceMismatch)
        );
    }
    assert_eq!(resolve(&value, "", &[]), Err(MappingError::InvalidSourceId));
}

#[test]
fn selector_names_nonempty_values_and_map_budgets_are_enforced() {
    assert!(HostBinding::new(HOST, "source-1", 1, Labels::new()).is_err());
    for name in [
        "",
        "1label",
        "instance=~",
        "a,b",
        "a}",
        "site-name",
        "a:b",
        "__name__",
        "сайт",
    ] {
        assert!(
            HostBinding::new(HOST, "source-1", 1, labels(&[(name, "x")])).is_err(),
            "{name}"
        );
    }
    assert!(HostBinding::new(HOST, "source-1", 1, labels(&[("instance", "")])).is_err());
    assert!(HostBinding::new(
        HOST,
        "source-1",
        1,
        labels(&[("instance", &"x".repeat(4097))])
    )
    .is_err());
    assert!(HostBinding::new(HOST, "source-1", 1, labels(&[(&"x".repeat(129), "a")])).is_err());
    let too_many = (0..33).map(|i| (format!("label{i}"), "x".into())).collect();
    assert!(HostBinding::new(HOST, "source-1", 1, too_many).is_err());
    let too_large = (0..5)
        .map(|i| (format!("label{i}"), "x".repeat(4096)))
        .collect();
    assert_eq!(
        HostBinding::new(HOST, "source-1", 1, too_large),
        Err(MappingError::BudgetExceeded)
    );
}

#[test]
fn promql_renders_only_valid_metric_and_sorted_exact_matchers() {
    let value = binding(labels(&[("job", "node"), ("instance", "node:9100")]));
    assert_eq!(
        value.render_metric("node_cpu_seconds_total").unwrap(),
        "node_cpu_seconds_total{instance=\"node:9100\",job=\"node\"}"
    );
    assert!(value.render_metric(":recorded:metric").is_ok());
    for metric in [
        "",
        "1metric",
        "up or vector(1)",
        "up{}",
        "up[5m]",
        "sum(up)",
        "up\n",
        "up,other",
        "utf8_я",
    ] {
        assert_eq!(
            value.render_metric(metric),
            Err(MappingError::InvalidMetric),
            "{metric}"
        );
    }
    assert!(value.render_metric(&"x".repeat(129)).is_err());
}

#[test]
fn json_label_escaping_prevents_operator_and_newline_injection() {
    let attacker = "node\"} or vector(1) #\\\n\r\t\0♥";
    let value = binding(labels(&[("instance", attacker)]));
    let query = value.render_metric("up").unwrap();
    assert_eq!(
        query,
        format!(
            "up{{instance={}}}",
            serde_json::to_string(attacker).unwrap()
        )
    );
    assert!(!query.contains('\n'));
    assert!(!query.contains('\0'));
    let escaped = query
        .strip_prefix("up{instance=")
        .unwrap()
        .strip_suffix('}')
        .unwrap();
    assert_eq!(serde_json::from_str::<String>(escaped).unwrap(), attacker);
    for byte in 0..=127u8 {
        let literal = format!("prefix{}suffix", byte as char);
        let rendered = binding(labels(&[("instance", &literal)]))
            .render_metric("up")
            .unwrap();
        let parsed = rendered
            .strip_prefix("up{instance=")
            .unwrap()
            .strip_suffix('}')
            .unwrap();
        assert_eq!(serde_json::from_str::<String>(parsed).unwrap(), literal);
    }
}

#[test]
fn escaped_query_budget_is_checked_after_expansion() {
    let control = "\0".repeat(4096);
    let value = binding(labels(&[("instance", &control)]));
    assert_eq!(value.render_metric("up"), Err(MappingError::BudgetExceeded));
    assert_eq!(
        HostBinding::from_json(&vec![b' '; MAX_BINDING_JSON_BYTES + 1]),
        Err(MappingError::BudgetExceeded)
    );
}

#[test]
fn fixed_panel_matchers_cannot_replace_or_repeat_explicit_identity_labels() {
    let value = binding(labels(&[("instance", "node")]));
    assert_eq!(
        value
            .render_metric_with_exact("node_cpu_seconds_total", &labels(&[("mode", "idle")]))
            .unwrap(),
        "node_cpu_seconds_total{instance=\"node\",mode=\"idle\"}"
    );
    for literal in ["node", "other"] {
        assert_eq!(
            value.render_metric_with_exact("up", &labels(&[("instance", literal)])),
            Err(MappingError::ConflictingMatcher)
        );
    }
    assert!(value
        .render_metric_with_exact("up", &labels(&[("bad=~", "x")]))
        .is_err());
    assert!(value
        .render_metric_with_exact("up", &labels(&[("mode", "")]))
        .is_err());
    let extra: Labels = (0..32).map(|i| (format!("label{i}"), "x".into())).collect();
    assert!(value.render_metric_with_exact("up", &extra).is_err());
    let attack = "idle\"} or vector(1) #";
    assert_eq!(
        value
            .render_metric_with_exact("up", &labels(&[("mode", attack)]))
            .unwrap(),
        format!(
            "up{{instance=\"node\",mode={}}}",
            serde_json::to_string(attack).unwrap()
        )
    );
    assert_eq!(
        value.render_metric_with_exact("up", &Labels::new()),
        value.render_metric("up")
    );
}

#[test]
fn identity_discovery_returns_missing_single_or_ambiguous_original_indices() {
    let value = binding(labels(&[("instance", "node"), ("job", "node_exporter")]));
    assert_eq!(
        resolve(&value, "source-1", &[]).unwrap(),
        Resolution::Missing
    );
    assert_eq!(
        resolve(&value, "source-1", &[labels(&[("instance", "node")])]).unwrap(),
        Resolution::Missing
    );
    let one = labels(&[
        ("instance", "node"),
        ("job", "node_exporter"),
        ("site", "dc1"),
    ]);
    let other = labels(&[("instance", "other"), ("job", "node_exporter")]);
    assert_eq!(
        resolve(&value, "source-1", &[other.clone(), one.clone()]).unwrap(),
        Resolution::Resolved { candidate_index: 1 }
    );
    assert_eq!(
        resolve(&value, "source-1", &[one.clone(), other, one]).unwrap(),
        Resolution::Ambiguous {
            candidate_indices: vec![0, 2]
        }
    );
}

#[test]
fn duplicate_identical_candidates_are_not_deduplicated_into_a_host() {
    let value = binding(labels(&[("instance", "node")]));
    let same = labels(&[("instance", "node")]);
    assert_eq!(
        resolve(&value, "source-1", &[same.clone(), same]).unwrap(),
        Resolution::Ambiguous {
            candidate_indices: vec![0, 1]
        }
    );
    assert_eq!(
        serde_json::to_value(Resolution::Resolved { candidate_index: 4 }).unwrap(),
        json!({"state":"resolved","candidateIndex":4})
    );
}

#[test]
fn received_host_id_ip_or_hostname_never_creates_implicit_binding_authority() {
    let value = binding(labels(&[("chosen_identity", "native-reviewed-label")]));
    let untrusted = labels(&[
        ("cc_host_id", HOST),
        ("instance", "127.0.0.1"),
        ("hostname", "node"),
        ("hostId", HOST),
    ]);
    assert_eq!(
        resolve(&value, "source-1", &[untrusted]).unwrap(),
        Resolution::Missing
    );
    // Only the explicitly selected label is compared; even a different claimed
    // cc_host_id neither grants nor overrides Native's mapping decision.
    let candidate = labels(&[
        ("chosen_identity", "native-reviewed-label"),
        ("cc_host_id", "other-host"),
    ]);
    assert_eq!(
        resolve(&value, "source-1", &[candidate]).unwrap(),
        Resolution::Resolved { candidate_index: 0 }
    );
}

#[test]
fn discovery_quotas_validate_all_candidates_instead_of_returning_first_match() {
    let value = binding(labels(&[("instance", "node")]));
    assert_eq!(
        resolve(&value, "source-1", &vec![Labels::new(); 1001]),
        Err(MappingError::BudgetExceeded)
    );
    let many = (0..65).map(|i| (format!("label{i}"), "x".into())).collect();
    assert_eq!(
        resolve(&value, "source-1", &[many]),
        Err(MappingError::BudgetExceeded)
    );
    for invalid in [
        labels(&[("", "value")]),
        labels(&[("label", &"x".repeat(4097))]),
        labels(&[(&"x".repeat(1025), "a")]),
    ] {
        assert_eq!(
            resolve(
                &value,
                "source-1",
                &[labels(&[("instance", "node")]), invalid]
            ),
            Err(MappingError::InvalidDiscovery)
        );
    }
    // 65 candidates * 64 labels * (4096 value bytes + key) exceeds 16 MiB.
    let fat: Labels = (0..64)
        .map(|i| (format!("label{i}"), "x".repeat(4096)))
        .collect();
    assert_eq!(
        resolve(&value, "source-1", &vec![fat; 65]),
        Err(MappingError::BudgetExceeded)
    );
}

#[test]
fn equality_is_literal_and_never_regex_substring_or_case_insensitive() {
    let value = binding(labels(&[("instance", "node.*")]));
    for instance in ["node01", "node", "NODE.*", "prefixnode.*", "node.*suffix"] {
        assert_eq!(
            resolve(&value, "source-1", &[labels(&[("instance", instance)])]).unwrap(),
            Resolution::Missing
        );
    }
    assert_eq!(
        resolve(&value, "source-1", &[labels(&[("instance", "node.*")])]).unwrap(),
        Resolution::Resolved { candidate_index: 0 }
    );
    assert_eq!(
        value.render_metric("up").unwrap(),
        "up{instance=\"node.*\"}"
    );
}

#[test]
fn trusted_native_enrollment_factory_uses_only_authorized_uuid_and_bounded_labels() {
    let native = Uuid::parse_str(HOST).unwrap();
    let enrollment = labels_for_trusted_enrollment(native, "site-a", "production").unwrap();
    assert_eq!(
        enrollment.as_labels(),
        &labels(&[
            ("cc_host_id", HOST),
            ("site", "site-a"),
            ("environment", "production")
        ])
    );
    for (site, environment) in [
        ("", "prod"),
        ("site", ""),
        ("site\n", "prod"),
        ("site", "prod\0"),
    ] {
        assert_eq!(
            labels_for_trusted_enrollment(native, site, environment),
            Err(MappingError::InvalidEnrollment)
        );
    }
    assert!(labels_for_trusted_enrollment(native, &"x".repeat(129), "prod").is_err());
    assert!(labels_for_trusted_enrollment(Uuid::nil(), "site", "prod").is_err());
    // Producing labels does not bypass the explicit selector or observed source.
    let value = binding(labels(&[("instance", "node")]));
    assert_eq!(
        resolve(&value, "source-1", &[enrollment.into_labels()]).unwrap(),
        Resolution::Missing
    );
}
