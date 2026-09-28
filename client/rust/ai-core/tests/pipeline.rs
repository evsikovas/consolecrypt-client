//! Search pipeline (exact/FTS → semantic → RAG → generation) and embedding
//! sync against an in-memory index.

mod common;

use cc_ai_core::context::{KbHit, StaticContext};
use cc_ai_core::pipeline::{AnswerOrigin, LlmMode, ResultOrigin, SearchOptions, StageKind};
use cc_ai_core::sanitizer::PrivacyProfile;
use cc_ai_core::{AiAssistant, EmbeddingSync};
use cc_models::ObjectId;
use cc_search_core::{DocKind, Document, SearchIndex};
use common::MockProvider;
use std::sync::Arc;

fn corpus() -> Vec<Document> {
    let d = |k, t: &str, b: &str, tags: &[&str]| {
        Document::new(ObjectId::new(), k, t, b).with_tags(tags)
    };
    vec![
        d(
            DocKind::Snippet,
            "Tail pod logs",
            "kubectl logs -n {{namespace}} {{pod}} --tail=100",
            &["k8s"],
        ),
        d(
            DocKind::Snippet,
            "Restart deployment",
            "kubectl rollout restart deploy/{{name}}",
            &["k8s"],
        ),
        d(
            DocKind::Snippet,
            "Disk usage",
            "du -sh /var/* | sort -h",
            &["linux"],
        ),
        d(
            DocKind::Note,
            "Postgres maintenance",
            "Run VACUUM ANALYZE weekly. PGPASSWORD=NoteSecret99 psql -c 'vacuum'",
            &["db"],
        ),
        d(
            DocKind::History,
            "systemctl restart nginx",
            "systemctl restart nginx",
            &[],
        ),
    ]
}

async fn setup(llm_responses: &[&str]) -> (Arc<MockProvider>, Arc<SearchIndex>, AiAssistant) {
    let mock = Arc::new(MockProvider::local(PrivacyProfile::Local));
    for r in llm_responses {
        mock.push(*r);
    }
    let index = Arc::new(SearchIndex::open_in_memory().unwrap());
    index.rebuild(corpus()).unwrap();
    let a = AiAssistant::new(mock.clone(), Arc::new(StaticContext::default()))
        .with_index(index.clone());
    (mock, index, a)
}

fn stage(o: &cc_ai_core::SearchOutcome, k: StageKind) -> &cc_ai_core::pipeline::StageReport {
    o.stages.iter().find(|s| s.stage == k).unwrap()
}

#[tokio::test]
async fn embedding_sync_and_model_change() {
    let (mock, index, _) = setup(&[]).await;
    let sync = EmbeddingSync::new(mock.clone(), index.clone()).with_batch_size(2);
    let s = sync.run(None).await.unwrap();
    assert!(s.model_changed);
    assert_eq!(s.embedded, 5);
    assert_eq!(s.remaining, 0);
    // Embedding inputs were sanitized.
    assert!(!mock.all_sent().contains("NoteSecret99"));
    let again = sync.run(None).await.unwrap();
    assert_eq!((again.embedded, again.model_changed), (0, false));

    // Another embedding model → vectors dropped and everything re-embedded.
    let other = Arc::new(MockProvider::with_embedding_model("other-embed"));
    let s = EmbeddingSync::new(other.clone(), index.clone())
        .run(None)
        .await
        .unwrap();
    assert!(s.model_changed);
    assert_eq!(s.embedded, 5);
    assert_eq!(
        index.embedding_model().unwrap().unwrap().id,
        "ollama:other-embed"
    );
    // Back to the first model → rebuilt again.
    let s = sync.run(None).await.unwrap();
    assert!(s.model_changed);
    assert_eq!(s.embedded, 5);
}

#[tokio::test]
async fn exact_match_skips_llm() {
    let (mock, index, a) = setup(&[]).await;
    EmbeddingSync::new(mock.clone(), index)
        .run(None)
        .await
        .unwrap();
    let before = mock.chats.lock().unwrap().len();
    let o = a
        .search("Tail pod logs", &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(o.hits[0].document.title, "Tail pod logs");
    assert!(matches!(
        o.hits[0].origin,
        ResultOrigin::Exact | ResultOrigin::Hybrid
    ));
    assert!(o.answer.is_none());
    assert!(stage(&o, StageKind::FullText).ran);
    assert!(stage(&o, StageKind::Semantic).ran);
    assert_eq!(
        stage(&o, StageKind::Rag).note.as_deref(),
        Some("exact local match")
    );
    assert_eq!(mock.chats.lock().unwrap().len(), before);
}

#[tokio::test]
async fn semantic_hits_then_rag_with_sources() {
    let (mock, index, a) = setup(&[r#"{"command": "kubectl logs -n prod api-1 --tail=50", "explanation": "adapted from [1]", "risk_suggestion": "read_only", "sources": [1]}"#]).await;
    EmbeddingSync::new(mock.clone(), index)
        .run(None)
        .await
        .unwrap();
    let o = a
        .search(
            "show kubernetes container output",
            &SearchOptions::default(),
        )
        .await
        .unwrap();
    assert!(!o.hits.is_empty());
    assert!(o
        .hits
        .iter()
        .any(|h| matches!(h.origin, ResultOrigin::Semantic | ResultOrigin::Hybrid)));
    assert!(
        o.hits[0].document.tags.contains(&"k8s".to_owned()),
        "{:?}",
        o.hits[0]
    );
    let ans = o.answer.clone().expect("RAG answer");
    assert_eq!(ans.origin, AnswerOrigin::Rag);
    assert_eq!(ans.sources, vec![o.hits[0].document.id]);
    assert_eq!(
        ans.proposal.suggestion.command,
        "kubectl logs -n prod api-1 --tail=50"
    );
    assert_eq!(ans.proposal.run.risk, cc_ai_core::RiskLevel::ReadOnly);
    assert!(stage(&o, StageKind::Rag).ran);
    assert!(!stage(&o, StageKind::Generation).ran);
    let sent = mock.chats.lock().unwrap().last().cloned().unwrap();
    assert!(sent.contains("Knowledge base entries:") && sent.contains("[1] "));
}

#[tokio::test]
async fn no_hits_goes_to_generation_and_never_mode_stays_local() {
    let (mock, _index, a) =
        setup(&[r#"{"command": "uptime", "explanation": "load", "risk_suggestion": "read_only"}"#])
            .await;
    let opts = SearchOptions {
        semantic: false,
        ..Default::default()
    };
    let o = a.search("zzqx frobnication", &opts).await.unwrap();
    assert!(o.hits.is_empty());
    let ans = o.answer.clone().unwrap();
    assert_eq!(ans.origin, AnswerOrigin::Generated);
    assert!(ans.sources.is_empty());
    assert!(stage(&o, StageKind::Generation).ran);
    assert_eq!(
        stage(&o, StageKind::Semantic).note.as_deref(),
        Some("disabled")
    );

    let calls = mock.chats.lock().unwrap().len();
    let o = a
        .search(
            "zzqx",
            &SearchOptions {
                llm: LlmMode::Never,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(o.answer.is_none());
    assert_eq!(mock.chats.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn semantic_skipped_until_embeddings_match_the_model() {
    let (_mock, _index, a) = setup(&[]).await;
    let o = a
        .search(
            "disk",
            &SearchOptions {
                llm: LlmMode::Never,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let s = stage(&o, StageKind::Semantic);
    assert!(!s.ran);
    assert!(s.note.as_deref().unwrap().contains("EmbeddingSync"));
    assert_eq!(o.hits[0].document.title, "Disk usage");
}

#[tokio::test]
async fn without_index_uses_context_kb_search() {
    let mock = Arc::new(MockProvider::remote(PrivacyProfile::Standard));
    let ctx = StaticContext {
        kb: vec![KbHit {
            id: ObjectId::new(),
            kind: DocKind::Note,
            title: "Rotate nginx logs".into(),
            body: "logrotate -f /etc/logrotate.d/nginx".into(),
            tags: vec![],
            score: 0.9,
        }],
        ..Default::default()
    };
    let a = AiAssistant::new(mock, Arc::new(ctx));
    let o = a
        .search(
            "nginx logs",
            &SearchOptions {
                llm: LlmMode::Never,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(o.hits.len(), 1);
    assert_eq!(o.hits[0].origin, ResultOrigin::FullText);
    assert!(!o.hits[0].exact);
    assert!(stage(&o, StageKind::FullText).note.is_some());
    assert!(a.search("  ", &SearchOptions::default()).await.is_err());
}
