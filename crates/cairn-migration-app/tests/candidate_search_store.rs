//! The candidate search loop against a real deployment store rather than an in-memory one.
//!
//! The aggregate's own tests prove its transitions. What they cannot prove is that a transition
//! survives the trip through a deployment: a real `SQLite` journal on a real path, opened per call,
//! off the runtime's own thread. That is the part of the loop that only fails once it is deployed,
//! so it is the part worth exercising here.

use std::error::Error;

use cairn_migration::{
    CandidateBudgetNoticeThreshold, CandidateBuildOutcomeV1, CandidateEmptySubmissionLimit,
    CandidateIterationLimit, CandidateProposalArtifact, CandidateRepeatWindow,
    CandidateSearchNextActionV1, CandidateSearchNoticeV1, CandidateSearchPolicyV1,
    CandidateSearchStopV1, CandidateSearchTerminalV1,
};
use cairn_migration_app::CandidateSearchStoreV1;
use cairn_protocol::{ContentId, TaskId};
use cairn_server::ServerConfig;

fn server(store_root: &std::path::Path) -> Result<ServerConfig, Box<dyn Error + Send + Sync>> {
    Ok(serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "listen": "127.0.0.1:0",
        "tls": {
            "certificate": "secrets/controller.pem",
            "private_key": "secrets/controller-key.pem",
            "client_ca": "secrets/ca.pem"
        },
        "enrollment_service": null,
        "protocol_version": 1,
        "scheduler": null,
        "handshake_timeout_ms": 10_000,
        "idle_timeout_ms": 120_000,
        "outbox_poll_interval_ms": 100,
        "authority_poll_interval_ms": 1_000,
        "diagnostic_byte_limit": 1_024,
        "store_root": store_root,
    }))?)
}

fn policy() -> Result<CandidateSearchPolicyV1, Box<dyn Error + Send + Sync>> {
    Ok(CandidateSearchPolicyV1 {
        iteration_limit: CandidateIterationLimit::new(2)?,
        empty_submission_limit: CandidateEmptySubmissionLimit::new(2)?,
        repeat_window: CandidateRepeatWindow::new(4)?,
        budget_notice_threshold: CandidateBudgetNoticeThreshold::new(1)?,
    })
}

fn proposal(
    label: &[u8],
) -> Result<ContentId<CandidateProposalArtifact>, Box<dyn Error + Send + Sync>> {
    Ok(ContentId::derive(label)?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_search_loop_advances_and_stops_through_a_deployment_store()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let store = CandidateSearchStoreV1::new(server(directory.path())?, policy()?);
    let task_id = TaskId::new();

    let opened = store.open(task_id)?;
    assert!(matches!(
        opened.next_action(),
        CandidateSearchNextActionV1::RequestProposal { parent: None, .. }
    ));

    // Opening again returns the position the loop already reached rather than a second loop.
    assert_eq!(store.open(task_id)?, opened);

    let first = proposal(b"first")?;
    store.record_proposal(task_id, first)?;
    let refused = store.record_build_observation(
        task_id,
        CandidateBuildOutcomeV1::new(first, ContentId::derive(b"receipt-one")?, false),
    )?;
    let CandidateSearchNextActionV1::RequestProposal {
        parent,
        remaining,
        notice,
        ..
    } = refused.next_action()
    else {
        panic!("a refused build asks for another proposal");
    };
    assert_eq!(parent.map(|parent| parent.proposal()), Some(first));
    assert_eq!(remaining.get(), 1);
    assert_eq!(
        notice,
        Some(CandidateSearchNoticeV1::BuildBudgetLow { remaining })
    );

    let second = proposal(b"second")?;
    store.record_proposal(task_id, second)?;
    let stopped = store.record_build_observation(
        task_id,
        CandidateBuildOutcomeV1::new(second, ContentId::derive(b"receipt-two")?, false),
    )?;
    assert!(matches!(
        stopped.next_action(),
        CandidateSearchNextActionV1::Terminal(CandidateSearchTerminalV1::Stopped {
            stop: CandidateSearchStopV1::IterationBudgetExhausted,
            ..
        })
    ));

    // A reader holding nothing but the store reaches the same place.
    assert_eq!(store.recover(task_id)?, stopped);
    Ok(())
}

// Two tasks share one deployment store. Their loops are separate aggregates, and a transition
// recorded for one must not be visible to the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_tasks_in_one_store_keep_separate_loops() -> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let store = CandidateSearchStoreV1::new(server(directory.path())?, policy()?);
    let first_task = TaskId::new();
    let second_task = TaskId::new();
    store.open(first_task)?;
    store.open(second_task)?;

    let only_for_first = proposal(b"only-for-first")?;
    store.record_proposal(first_task, only_for_first)?;

    assert!(matches!(
        store.recover(first_task)?.next_action(),
        CandidateSearchNextActionV1::RequestBuild { .. }
    ));
    assert!(matches!(
        store.recover(second_task)?.next_action(),
        CandidateSearchNextActionV1::RequestProposal { parent: None, .. }
    ));
    Ok(())
}
