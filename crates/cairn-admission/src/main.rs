//! One-shot, model-free Admission process commands.

use std::{env, io::Write, str::FromStr};

use cairn_migration::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputV1, SirIntentHypothesisSetProposalArtifact,
    derive_user_intent_decision_requests,
};
use cairn_protocol::{ContentId, ContentType};
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use serde::de::DeserializeOwned;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().ok_or(usage())?;
    let database = arguments.next().ok_or(usage())?;
    let cas = arguments.next().ok_or(usage())?;
    let proposal_wire = arguments.next().ok_or(usage())?;
    if command != "intent-decision-requests" || arguments.next().is_some() {
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
    "usage: cairn-admission intent-decision-requests <public-content.db> <public-cas> <proposal-id>"
}
