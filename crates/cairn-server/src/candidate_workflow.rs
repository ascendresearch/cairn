//! Controller composition for the product-owned Candidate native-build workflow seam.

use std::io::Cursor;

use cairn_execution::{DockerImageId, JobContract, JobContractArtifact};
use cairn_migration::{
    CandidateBuildEnvironmentProfileV1, CandidateNativeBuildDispatchV1,
    CandidateNativeBuildScheduleV1, CandidateNativePublicationV1,
    CollectionCandidateNativeFollowupRevisionArtifact,
    CollectionCandidateNativeRepairRevisionArtifact, CollectionCandidateRevisionArtifact,
    prepare_candidate_native_followup_build_job, prepare_candidate_native_repair_build_job,
    prepare_candidate_native_revision_build_job,
};
use cairn_protocol::{ContentId, ContentType, JobId};
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;

use crate::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome, ServerConfig,
    ServerError, schedule_execution_contract,
};

/// Materializes and archives the exact native-build contract selected by durable workflow state.
///
/// This performs no scheduling or Worker communication. The returned dispatch must be committed
/// to the migration workflow before [`schedule_candidate_native_build`] is called.
///
/// # Errors
///
/// Returns an error when configuration/storage fails, the selected publication is absent or
/// corrupt, or native-build material cannot be prepared under its typed publication domain.
pub fn prepare_candidate_native_build_dispatch(
    config: &ServerConfig,
    publication: CandidateNativePublicationV1,
    job_id: JobId,
    image: DockerImageId,
    profile: CandidateBuildEnvironmentProfileV1,
    schedule: CandidateNativeBuildScheduleV1,
) -> Result<CandidateNativeBuildDispatchV1, ServerError> {
    config.validate_schema()?;
    let mut content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .map_err(workflow_error)?;

    match publication {
        CandidateNativePublicationV1::Revision(publication_id) => {
            let bytes =
                load_content::<CollectionCandidateRevisionArtifact>(&content, publication_id)?;
            let prepared = prepare_candidate_native_revision_build_job(
                job_id,
                &bytes,
                publication_id,
                image,
                profile,
            )
            .map_err(workflow_error)?;
            prepared
                .archive_materials(&mut content)
                .map_err(workflow_error)?;
            archive_contract(
                &mut content,
                prepared.contract_bytes(),
                prepared.contract_id(),
            )?;
            Ok(CandidateNativeBuildDispatchV1::new(
                publication,
                job_id,
                prepared.input_bundle_id(),
                prepared.environment_id(),
                prepared.contract_id(),
                schedule,
            ))
        }
        CandidateNativePublicationV1::NativeFollowup(publication_id) => {
            let bytes = load_content::<CollectionCandidateNativeFollowupRevisionArtifact>(
                &content,
                publication_id,
            )?;
            let prepared = prepare_candidate_native_followup_build_job(
                job_id,
                &bytes,
                publication_id,
                image,
                profile,
            )
            .map_err(workflow_error)?;
            prepared
                .archive_materials(&mut content)
                .map_err(workflow_error)?;
            archive_contract(
                &mut content,
                prepared.contract_bytes(),
                prepared.contract_id(),
            )?;
            Ok(CandidateNativeBuildDispatchV1::new(
                publication,
                job_id,
                prepared.input_bundle_id(),
                prepared.environment_id(),
                prepared.contract_id(),
                schedule,
            ))
        }
        CandidateNativePublicationV1::NativeRepair(publication_id) => {
            let bytes = load_content::<CollectionCandidateNativeRepairRevisionArtifact>(
                &content,
                publication_id,
            )?;
            let prepared = prepare_candidate_native_repair_build_job(
                job_id,
                &bytes,
                publication_id,
                image,
                profile,
            )
            .map_err(workflow_error)?;
            prepared
                .archive_materials(&mut content)
                .map_err(workflow_error)?;
            archive_contract(
                &mut content,
                prepared.contract_bytes(),
                prepared.contract_id(),
            )?;
            Ok(CandidateNativeBuildDispatchV1::new(
                publication,
                job_id,
                prepared.input_bundle_id(),
                prepared.environment_id(),
                prepared.contract_id(),
                schedule,
            ))
        }
    }
}

/// Schedules only the exact contract and identities previously committed in a workflow dispatch.
///
/// # Errors
///
/// Returns an error when the contract is absent, corrupt, noncanonical, changes any dispatch
/// binding, or the ordinary Controller scheduler rejects the request.
pub fn schedule_candidate_native_build(
    config: &ServerConfig,
    dispatch: &CandidateNativeBuildDispatchV1,
) -> Result<ControllerSchedulingOutcome, ServerError> {
    config.validate_schema()?;
    let content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .map_err(workflow_error)?;
    let bytes = load_content::<JobContractArtifact>(&content, dispatch.contract())?;
    let contract: JobContract = cairn_codec::from_slice(&bytes).map_err(workflow_error)?;
    if cairn_codec::to_vec(&contract).map_err(workflow_error)? != bytes
        || contract.job_id() != dispatch.job_id()
        || contract.input_bundle_id() != dispatch.input_bundle()
        || contract.environment_id() != dispatch.environment()
    {
        return Err(ServerError::MigrationWorkflow(
            "native-build dispatch changed its archived contract binding".into(),
        ));
    }
    schedule_execution_contract(config, &contract, schedule_ids(dispatch.schedule()))
}

fn schedule_ids(ids: CandidateNativeBuildScheduleV1) -> ControllerScheduleIds {
    ControllerScheduleIds {
        attempt_id: ids.attempt_id,
        placement_id: ids.placement_id,
        reservation_id: ids.reservation_id,
        assignment_id: ids.assignment_id,
        lease_id: ids.lease_id,
        offer_message_id: ids.offer_message_id,
        start_message_id: ids.start_message_id,
        commands: ControllerScheduleCommandIds {
            authorize_attempt: ids.authorize_attempt_command,
            reserve_placement: ids.reserve_placement_command,
            grant_assignment: ids.grant_assignment_command,
            enqueue_offer: ids.enqueue_offer_command,
        },
    }
}

fn load_content<T: ContentType>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<Vec<u8>, ServerError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(workflow_error)?;
    Ok(bytes)
}

fn archive_contract(
    content: &mut SqliteContentStore,
    bytes: &[u8],
    expected: ContentId<JobContractArtifact>,
) -> Result<(), ServerError> {
    let actual = content
        .put::<JobContractArtifact>(&mut Cursor::new(bytes))
        .map_err(workflow_error)?
        .content_id;
    if actual != expected {
        return Err(ServerError::MigrationWorkflow(
            "native-build contract identity changed during archival".into(),
        ));
    }
    Ok(())
}

fn workflow_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}
