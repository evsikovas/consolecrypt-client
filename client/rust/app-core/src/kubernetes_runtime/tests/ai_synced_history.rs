use super::*;
use cc_models::ai::{AiConversation, ChatMessage, ChatRole};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synced_kubernetes_chat_is_context_scoped_and_remote_changes_revoke_consent() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    ai_provider(&app, "https://model.example.invalid/v1").await;
    let now = chrono::Utc::now();
    let mut conversation = AiConversation {
        id: ObjectId::new(),
        title: "Previous device".into(),
        provider_id: None,
        kubernetes_context_id: Some(context.id),
        messages: vec![
            ChatMessage {
                role: ChatRole::User,
                content: "What is pending?".into(),
                created_at: now,
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "The Pod cannot be scheduled.".into(),
                created_at: now,
            },
        ],
        created_at: now,
        updated_at: now,
    };
    let (owner, unlocked) = app.unlocked().await.unwrap();
    // A sync receiver projects the same authenticated decrypted VaultObject.
    // There is no device-local history record on this receiving device.
    unlocked
        .writer
        .put(VaultObject::AiConversation(conversation.clone()))
        .await
        .unwrap();
    let local: Option<Value> = owner
        .storage()
        .unwrap()
        .setting_get("kubernetes.ai.history.v1")
        .await
        .unwrap();
    assert!(local.is_none());
    let history = app.kube_ai_history(context.id.to_string()).await.unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].synced);
    assert_eq!(history[0].conversation.id, conversation.id);
    assert!(app.ai_list_conversations().await.unwrap().is_empty());
    assert!(app
        .ai_ask(
            None,
            crate::dto::AiAskRequestDto {
                conversation_id: Some(conversation.id.to_string()),
                question: "Resume outside the cluster".into(),
            },
            Default::default(),
        )
        .await
        .is_err());
    let mut other = context.clone();
    other.id = ObjectId::new();
    other.name = "Other context".into();
    app.save_kube_context(other.clone()).await.unwrap();
    assert!(app
        .kube_ai_history(other.id.to_string())
        .await
        .unwrap()
        .is_empty());
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let mut question = ai_question();
    question.conversation_id = Some(conversation.id.to_string());
    let review = app
        .kube_ai_prepare(session.id.clone(), question)
        .await
        .unwrap();
    assert_eq!(review.history_messages, 2);
    conversation.updated_at += chrono::Duration::seconds(1);
    conversation.messages[1].content = "A later answer from another device.".into();
    unlocked
        .writer
        .put(VaultObject::AiConversation(conversation.clone()))
        .await
        .unwrap();
    assert!(app
        .kube_ai_start(session.id.clone(), review.ticket, true)
        .await
        .is_err());
    app.kube_ai_delete_conversation(conversation.id.to_string())
        .await
        .unwrap();
    assert!(app
        .kube_ai_history(context.id.to_string())
        .await
        .unwrap()
        .is_empty());

    // Existing conversation payloads remain valid with no context association.
    let mut old_payload = serde_json::to_value(&conversation).unwrap();
    old_payload
        .as_object_mut()
        .unwrap()
        .remove("kubernetes_context_id");
    assert!(serde_json::from_value::<AiConversation>(old_payload)
        .unwrap()
        .kubernetes_context_id
        .is_none());
    app.shutdown().await.unwrap();
}
