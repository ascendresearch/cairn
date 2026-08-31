//! One-shot, model-free Admission process commands.

use std::{
    env,
    io::{Cursor, Write},
    str::FromStr,
};

use cairn_admission::{
    RestrictedIntentAdmissionDecisionArtifact, UserIntentAuthorityGrantV1,
    UserIntentDecisionArtifact, UserIntentDecisionV1, promote_user_intent,
};
use cairn_migration::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputV1, MigrationIntentContractArtifact,
    SirIntentHypothesisSetProposalArtifact, UserIntentDecisionRequestV1,
    derive_user_intent_decision_requests,
};
use cairn_protocol::{ContentId, ContentType};
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use serde::de::DeserializeOwned;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().ok_or(usage())?;
    match command.as_str() {
        "intent-decision-requests" => decision_requests(arguments),
        "promote-user-intent" => promote(arguments),
        _ => Err(usage().into()),
    }
}

fn decision_requests(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = arguments.next().ok_or(usage())?;
    let cas = arguments.next().ok_or(usage())?;
    let proposal_wire = arguments.next().ok_or(usage())?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let proposal_id =
        ContentId::<SirIntentHypothesisSetProposalArtifact>::from_str(&proposal_wire)?;
    let store = SqliteContentStore::open_read_only(database, cas)?;
    let proposal: IntentHypothesisSetProposalV1 = load(&store, &proposal_id)?;
    let recovery_input_id = proposal.recovery_input();
    let recovery_input: IntentRecoveryInputV1 = load(&store, &recovery_input_id)?;
    let batch = derive_user_intent_decision_requests(
        proposal_id,
        &proposal,
        recovery_input_id,
        &recovery_input,
    )?;
    std::io::stdout().write_all(&cairn_codec::to_vec(&batch)?)?;
    Ok(())
}

fn promote(mut arguments: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let public_database = arguments.next().ok_or(usage())?;
    let public_cas = arguments.next().ok_or(usage())?;
    let restricted_database = arguments.next().ok_or(usage())?;
    let restricted_cas = arguments.next().ok_or(usage())?;
    let decision_wire = arguments.next().ok_or(usage())?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let decision_id = ContentId::<UserIntentDecisionArtifact>::from_str(&decision_wire)?;
    let public = SqliteContentStore::open_read_only(public_database, public_cas)?;
    let decision: UserIntentDecisionV1 = load(&public, &decision_id)?;
    let request_id = decision.request();
    let request: UserIntentDecisionRequestV1 = load(&public, &request_id)?;
    let grant_id = decision.authority_grant();
    let grant: UserIntentAuthorityGrantV1 = load(&public, &grant_id)?;
    let proposal_id = request.proposal();
    let proposal: IntentHypothesisSetProposalV1 = load(&public, &proposal_id)?;
    let recovery_input_id = request.recovery_input();
    let recovery_input: IntentRecoveryInputV1 = load(&public, &recovery_input_id)?;
    let prepared = promote_user_intent(
        proposal_id,
        &proposal,
        recovery_input_id,
        &recovery_input,
        request_id,
        &request,
        grant_id,
        &grant,
        decision_id,
        &decision,
    )?;

    let mut restricted = SqliteContentStore::open(restricted_database, restricted_cas)?;
    let contract = prepared.public_outcome().contract();
    let archived_contract = restricted
        .put::<MigrationIntentContractArtifact>(&mut Cursor::new(cairn_codec::to_vec(contract)?))?
        .content_id;
    if archived_contract != contract.identity()? {
        return Err("restricted contract identity changed while archiving".into());
    }
    let archived_decision = restricted
        .put::<RestrictedIntentAdmissionDecisionArtifact>(&mut Cursor::new(cairn_codec::to_vec(
            prepared.restricted_decision(),
        )?))?
        .content_id;
    if archived_decision != prepared.public_outcome().restricted_decision() {
        return Err("restricted decision identity changed while archiving".into());
    }
    std::io::stdout().write_all(&cairn_codec::to_vec(prepared.public_outcome())?)?;
    Ok(())
}

fn load<T, V>(
    store: &SqliteContentStore,
    content_id: &ContentId<T>,
) -> Result<V, Box<dyn std::error::Error>>
where
    T: ContentType,
    V: DeserializeOwned,
{
    let mut bytes = Vec::new();
    store.write_to(content_id, &mut bytes)?;
    Ok(cairn_codec::from_slice(&bytes)?)
}

const fn usage() -> &'static str {
    "usage: cairn-admission intent-decision-requests <public-content.db> <public-cas> <proposal-id>\n       cairn-admission promote-user-intent <public-content.db> <public-cas> <restricted-content.db> <restricted-cas> <decision-id>"
}
