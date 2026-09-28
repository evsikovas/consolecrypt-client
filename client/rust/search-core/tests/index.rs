use cc_models::ObjectId;
use cc_search_core::*;

fn doc(kind: DocKind, title: &str, body: &str, tags: &[&str]) -> Document {
    Document::new(ObjectId::new(), kind, title, body).with_tags(tags)
}

fn corpus() -> Vec<Document> {
    vec![
        doc(
            DocKind::Snippet,
            "Tail pod logs",
            "kubectl logs -n {{namespace}} {{pod}} --tail={{lines}}",
            &["k8s", "logs"],
        ),
        doc(
            DocKind::Snippet,
            "Restart deployment",
            "kubectl rollout restart deployment/{{name}} -n {{namespace}}",
            &["k8s"],
        ),
        doc(
            DocKind::Snippet,
            "Disk usage",
            "du -sh /var/log/* | sort -h",
            &["linux", "disk"],
        ),
        doc(
            DocKind::Note,
            "Postgres vacuum",
            "Run VACUUM ANALYZE after bulk loads; check pg_stat_user_tables. Logs are in /var/log/postgresql.",
            &["db", "postgres"],
        ),
        doc(
            DocKind::History,
            "journalctl -u nginx --since today",
            "journalctl -u nginx --since today",
            &[],
        ),
        doc(
            DocKind::Note,
            "Перезапуск сервиса",
            "systemctl restart nginx — перезапуск веб-сервера",
            &["linux"],
        ),
    ]
}

fn index_with_corpus() -> (SearchIndex, Vec<Document>) {
    let idx = SearchIndex::open_in_memory().unwrap();
    let c = corpus();
    idx.rebuild(c.clone()).unwrap();
    (idx, c)
}

fn titles(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|h| h.document.title.as_str()).collect()
}

#[test]
fn fresh_index_needs_rebuild_until_rebuilt() {
    let idx = SearchIndex::open_in_memory().unwrap();
    assert!(idx.needs_rebuild().unwrap());
    assert!(idx.is_empty().unwrap());
    idx.rebuild(corpus()).unwrap();
    assert!(!idx.needs_rebuild().unwrap());
    assert_eq!(idx.len().unwrap(), 6);
}

#[test]
fn title_matches_outrank_body_matches() {
    let (idx, _) = index_with_corpus();
    // "logs" is in the title of "Tail pod logs" but only in the body of the
    // Postgres note.
    let hits = idx.search_text(&TextQuery::new("logs", 10)).unwrap();
    assert_eq!(hits[0].document.title, "Tail pod logs");
    assert!(titles(&hits).contains(&"Postgres vacuum"));
    assert!(hits[0].score >= hits[1].score);
    assert!(hits.iter().all(|h| h.matched.fts_rank.is_some()));
}

#[test]
fn prefix_search_and_multi_term() {
    let (idx, _) = index_with_corpus();
    let hits = idx.search_text(&TextQuery::new("kub rest", 10)).unwrap();
    assert_eq!(titles(&hits), vec!["Restart deployment"]);
    // Without prefix matching "kub" matches nothing.
    let none = idx
        .search_text(&TextQuery::new("kub", 10).with_prefix(false))
        .unwrap();
    assert!(none.is_empty());
}

#[test]
fn all_then_any_falls_back() {
    let (idx, _) = index_with_corpus();
    let all = idx
        .search_text(&TextQuery::new("kubectl vacuum", 10).with_mode(MatchMode::All))
        .unwrap();
    assert!(all.is_empty());
    let fallback = idx
        .search_text(&TextQuery::new("kubectl vacuum", 10))
        .unwrap();
    assert!(fallback.len() >= 3);
}

#[test]
fn tag_and_kind_filters() {
    let (idx, _) = index_with_corpus();
    let k8s_logs = idx
        .search_text(
            &TextQuery::new("kubectl", 10)
                .with_filter(SearchFilter::default().with_tags(["K8s", "logs"])),
        )
        .unwrap();
    assert_eq!(titles(&k8s_logs), vec!["Tail pod logs"]);

    let notes = idx
        .search_text(&TextQuery::new("log", 10).with_filter(SearchFilter::kinds([DocKind::Note])))
        .unwrap();
    assert_eq!(titles(&notes), vec!["Postgres vacuum"]);

    // Empty text + filter lists by title.
    let linux = idx
        .search_text(
            &TextQuery::new("", 10).with_filter(SearchFilter::default().with_tags(["linux"])),
        )
        .unwrap();
    assert_eq!(titles(&linux), vec!["Disk usage", "Перезапуск сервиса"]);
    // Empty text, no filter → nothing.
    assert!(idx
        .search_text(&TextQuery::new("  ", 10))
        .unwrap()
        .is_empty());
}

#[test]
fn exact_matches_come_first() {
    let (idx, _) = index_with_corpus();
    let hits = idx
        .search_text(&TextQuery::new("journalctl -u nginx --since today", 10))
        .unwrap();
    assert_eq!(hits[0].document.kind, DocKind::History);
    assert!(hits[0].matched.exact);
    assert_eq!(hits[0].matched.source(), HitSource::Exact);
}

#[test]
fn fts_syntax_in_user_input_is_inert() {
    let (idx, _) = index_with_corpus();
    for q in [
        "\"",
        "NEAR(",
        "kubectl*",
        "a OR b AND NOT c",
        "title:logs",
        "^logs",
        "{{pod}}",
        "'; DROP TABLE documents; --",
    ] {
        idx.search_text(&TextQuery::new(q, 10))
            .unwrap_or_else(|e| panic!("query {q:?} failed: {e}"));
    }
    assert_eq!(idx.len().unwrap(), 6);
    // Punctuation-only queries use a substring scan.
    let pipes = idx.search_text(&TextQuery::new("|", 10)).unwrap();
    assert_eq!(titles(&pipes), vec!["Disk usage"]);
}

#[test]
fn unicode_and_diacritics() {
    let (idx, _) = index_with_corpus();
    let hits = idx.search_text(&TextQuery::new("перезапуск", 10)).unwrap();
    assert_eq!(titles(&hits), vec!["Перезапуск сервиса"]);
    let idx2 = SearchIndex::open_in_memory().unwrap();
    idx2.upsert(&doc(DocKind::Note, "Café résumé", "naïve", &[]))
        .unwrap();
    assert_eq!(
        idx2.search_text(&TextQuery::new("cafe resume", 5))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn upsert_update_delete_get() {
    let idx = SearchIndex::open_in_memory().unwrap();
    let mut d = doc(DocKind::Snippet, "List pods", "kubectl get pods", &["k8s"]);
    assert_eq!(idx.upsert(&d).unwrap(), UpsertOutcome::Inserted);
    assert_eq!(idx.upsert(&d).unwrap(), UpsertOutcome::Unchanged);
    d.body = "kubectl get pods -A".into();
    d.tags = vec!["k8s".into(), "cluster".into()];
    assert_eq!(idx.upsert(&d).unwrap(), UpsertOutcome::Updated);
    assert_eq!(idx.get(d.id).unwrap().unwrap().body, "kubectl get pods -A");
    // Old FTS content is gone, new content is searchable.
    let q =
        TextQuery::new("cluster", 5).with_filter(SearchFilter::default().with_tags(["cluster"]));
    assert_eq!(idx.search_text(&q).unwrap().len(), 1);
    assert!(idx.delete(d.id).unwrap());
    assert!(!idx.delete(d.id).unwrap());
    assert!(idx.get(d.id).unwrap().is_none());
    assert!(idx
        .search_text(&TextQuery::new("pods", 5))
        .unwrap()
        .is_empty());
    assert_eq!(idx.upsert_many(&corpus()).unwrap(), 6);
}

#[test]
fn rebuild_replaces_corpus_and_keeps_unchanged_embeddings() {
    let (idx, mut c) = index_with_corpus();
    idx.set_embedding_model(&EmbeddingModel::new("test:m", 3))
        .unwrap();
    let jobs = idx.pending_embeddings(100).unwrap();
    assert_eq!(jobs.len(), 6);
    let items: Vec<_> = jobs
        .iter()
        .enumerate()
        .map(|(i, j)| {
            (
                j.document.id,
                j.content_hash.clone(),
                vec![1.0, i as f32, 0.5],
            )
        })
        .collect();
    assert_eq!(idx.store_embeddings(&items).unwrap(), 6);
    assert_eq!(
        idx.embedding_stats().unwrap(),
        EmbeddingStats {
            documents: 6,
            embedded: 6
        }
    );

    // Change one doc, drop one, add one.
    c[0].body.push_str(" --timestamps");
    let removed = c.remove(1);
    c.push(doc(DocKind::Snippet, "New one", "echo hi", &[]));
    let stats = idx.rebuild(c).unwrap();
    assert_eq!(stats.documents, 6);
    assert_eq!(stats.embeddings_kept, 4);
    assert!(idx.get(removed.id).unwrap().is_none());
    assert!(idx
        .search_text(&TextQuery::new("rollout", 5))
        .unwrap()
        .is_empty());
    let pending = idx.pending_embeddings(100).unwrap();
    let mut pending_titles: Vec<_> = pending.iter().map(|j| j.document.title.as_str()).collect();
    pending_titles.sort();
    assert_eq!(pending_titles, vec!["New one", "Tail pod logs"]);
}

#[test]
fn embedding_model_change_triggers_reembedding() {
    let (idx, _) = index_with_corpus();
    assert!(matches!(
        idx.pending_embeddings(10),
        Err(SearchError::NoEmbeddingModel)
    ));
    let m1 = EmbeddingModel::new("ollama:nomic-embed-text", 2);
    assert!(idx.set_embedding_model(&m1).unwrap());
    assert!(!idx.set_embedding_model(&m1).unwrap());
    for j in idx.pending_embeddings(100).unwrap() {
        assert!(idx
            .store_embedding(j.document.id, &j.content_hash, &[1.0, 0.0])
            .unwrap());
    }
    assert!(idx.pending_embeddings(100).unwrap().is_empty());
    let m2 = EmbeddingModel::new("ollama:mxbai-embed-large", 4);
    assert!(idx.set_embedding_model(&m2).unwrap());
    assert_eq!(idx.embedding_model().unwrap(), Some(m2));
    assert_eq!(idx.pending_embeddings(100).unwrap().len(), 6);
    assert_eq!(idx.embedding_stats().unwrap().embedded, 0);
    assert!(idx
        .set_embedding_model(&EmbeddingModel::new("", 4))
        .is_err());
    assert!(idx
        .set_embedding_model(&EmbeddingModel::new("x", 0))
        .is_err());
}

#[test]
fn store_embedding_validation_and_staleness() {
    let idx = SearchIndex::open_in_memory().unwrap();
    let mut d = doc(DocKind::Note, "n", "body", &[]);
    idx.upsert(&d).unwrap();
    idx.set_embedding_model(&EmbeddingModel::new("m", 2))
        .unwrap();
    let job = idx.pending_embeddings(1).unwrap().remove(0);
    assert!(matches!(
        idx.store_embedding(d.id, &job.content_hash, &[1.0]),
        Err(SearchError::DimensionMismatch {
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        idx.store_embedding(d.id, &job.content_hash, &[f32::NAN, 1.0]),
        Err(SearchError::NonFiniteEmbedding)
    ));
    // Document changes before the vector arrives → store is a no-op.
    d.body = "changed".into();
    idx.upsert(&d).unwrap();
    assert!(!idx
        .store_embedding(d.id, &job.content_hash, &[1.0, 0.0])
        .unwrap());
    assert!(!idx
        .store_embedding(ObjectId::new(), &job.content_hash, &[1.0, 0.0])
        .unwrap());
    assert_eq!(idx.pending_embeddings(10).unwrap().len(), 1);
}

/// Deterministic toy "embedding": counts of a few concept words.
fn toy_embed(text: &str) -> Vec<f32> {
    let t = text.to_lowercase();
    let concepts: [&[&str]; 4] = [
        &["kubectl", "pod", "k8s", "deployment", "kubernetes"],
        &["log", "journal", "tail"],
        &["disk", "du ", "space", "storage"],
        &["postgres", "vacuum", "database", "sql"],
    ];
    concepts
        .iter()
        .map(|words| words.iter().filter(|w| t.contains(*w)).count() as f32 + 0.01)
        .collect()
}

fn embed_all(idx: &SearchIndex) {
    idx.set_embedding_model(&EmbeddingModel::new("toy", 4))
        .unwrap();
    for j in idx.pending_embeddings(1000).unwrap() {
        idx.store_embedding(j.document.id, &j.content_hash, &toy_embed(&j.text()))
            .unwrap();
    }
}

#[test]
fn vector_search_topk_and_filters() {
    let (idx, _) = index_with_corpus();
    embed_all(&idx);
    let q = toy_embed("free storage space");
    let hits = idx
        .search_vector(&q, &SearchFilter::default(), 2, 0.0)
        .unwrap();
    assert_eq!(hits[0].document.title, "Disk usage");
    assert_eq!(hits.len(), 2);
    assert!(hits[0].score >= hits[1].score);
    assert_eq!(hits[0].matched.source(), HitSource::Semantic);

    let notes = idx
        .search_vector(&q, &SearchFilter::kinds([DocKind::Note]), 10, 0.0)
        .unwrap();
    assert!(notes.iter().all(|h| h.document.kind == DocKind::Note));
    let tagged = idx
        .search_vector(&q, &SearchFilter::default().with_tags(["k8s"]), 10, 0.0)
        .unwrap();
    assert_eq!(tagged.len(), 2);
    assert!(matches!(
        idx.search_vector(&[1.0], &SearchFilter::default(), 2, 0.0),
        Err(SearchError::DimensionMismatch { .. })
    ));
    // Deleting a document removes it from vector results (cache invalidated).
    let disk = hits[0].document.id;
    idx.delete(disk).unwrap();
    let hits = idx
        .search_vector(&q, &SearchFilter::default(), 10, 0.0)
        .unwrap();
    assert!(hits.iter().all(|h| h.document.id != disk));
}

#[test]
fn hybrid_fuses_text_and_semantic() {
    let (idx, _) = index_with_corpus();
    // Without embeddings: behaves like text search.
    let text_only = idx
        .search_hybrid(&HybridQuery::new("kubectl", Some(toy_embed("kubectl")), 10))
        .unwrap();
    assert_eq!(text_only.len(), 2);

    embed_all(&idx);
    // "kubernetes" never appears verbatim → found only semantically.
    let q = "kubernetes pod";
    let hits = idx
        .search_hybrid(&HybridQuery::new(q, Some(toy_embed(q)), 10).with_min_similarity(0.5))
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].document.tags.contains(&"k8s".to_owned()));
    let top = &hits[0];
    assert_eq!(top.matched.source(), HitSource::Hybrid, "{top:?}");

    // Semantic-only hit shows up with its source.
    let q = "database maintenance";
    let hits = idx
        .search_hybrid(&HybridQuery::new(q, Some(toy_embed(q)), 3).with_min_similarity(0.5))
        .unwrap();
    assert_eq!(hits[0].document.title, "Postgres vacuum");
    assert!(hits[0].matched.vector_rank.is_some());

    // Scores are descending.
    assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
}

#[test]
fn file_index_persists_and_is_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("search.db");
    let key = IndexKey::from_bytes(&[7u8; 32]);
    assert_eq!(format!("{key:?}"), "IndexKey(<redacted>)");
    {
        let idx = SearchIndex::open(&path, Some(&key)).unwrap();
        idx.rebuild(corpus()).unwrap();
    }
    {
        let idx = SearchIndex::open(&path, Some(&key)).unwrap();
        assert!(!idx.needs_rebuild().unwrap());
        assert_eq!(idx.len().unwrap(), 6);
        assert_eq!(
            idx.search_text(&TextQuery::new("vacuum", 5)).unwrap().len(),
            1
        );
    }
    // The file is not plaintext SQLite and does not leak content.
    let raw = std::fs::read(&path).unwrap();
    assert!(!raw.starts_with(b"SQLite format 3"));
    assert!(!raw.windows(6).any(|w| w == b"VACUUM"));

    let wrong = IndexKey::from_bytes(&[8u8; 32]);
    assert!(matches!(
        SearchIndex::open(&path, Some(&wrong)),
        Err(SearchError::Unreadable)
    ));
    assert!(matches!(
        SearchIndex::open(&path, None),
        Err(SearchError::Unreadable)
    ));

    let idx = SearchIndex::open_or_recreate(&path, Some(&wrong)).unwrap();
    assert!(idx.needs_rebuild().unwrap());
    assert!(idx.is_empty().unwrap());
}

#[test]
fn schema_version_change_recreates_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("search.db");
    {
        let idx = SearchIndex::open(&path, None).unwrap();
        idx.rebuild(corpus()).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 999;").unwrap();
    }
    let idx = SearchIndex::open(&path, None).unwrap();
    assert!(idx.needs_rebuild().unwrap());
    assert!(idx.is_empty().unwrap());
    idx.rebuild(corpus()).unwrap();
    assert_eq!(idx.len().unwrap(), 6);
}

#[test]
fn documents_from_domain_models() {
    use cc_models::snippet::{RiskLevel, Snippet, SnippetSource, SnippetType};
    let now = chrono::Utc::now();
    let s = Snippet {
        package_name: None,
        catalog_id: None,
        id: ObjectId::new(),
        name: "Pod logs".into(),
        description: "Follow logs".into(),
        snippet_type: SnippetType::Kubectl,
        shell: None,
        template: "kubectl logs -f {{pod}}".into(),
        variables: vec![],
        tags: vec!["K8s".into()],
        risk_level: RiskLevel::ReadOnly,
        source: SnippetSource::User,
        created_by: None,
        created_at: now,
        updated_at: now,
        last_used_at: None,
        usage_count: 0,
    };
    let d = Document::from_snippet(&s);
    assert_eq!(d.kind, DocKind::Snippet);
    assert!(d.body.contains("kubectl logs -f"));
    assert_eq!(d.tags, vec!["k8s", "kubectl"]);

    let mut h = cc_models::host::Host::new("prod-db", "10.0.0.5");
    h.username = Some("alice".into());
    h.tags = vec!["prod".into()];
    let d = Document::from_host(&h);
    assert_eq!(d.title, "prod-db");
    assert!(d.body.contains("10.0.0.5") && d.body.contains("alice"));

    let e = cc_models::history::HistoryEntry {
        id: ObjectId::new(),
        host_id: None,
        command: "false".into(),
        exit_code: Some(1),
        executed_at: now,
    };
    assert_eq!(Document::from_history(&e).tags, vec!["failed"]);
}
