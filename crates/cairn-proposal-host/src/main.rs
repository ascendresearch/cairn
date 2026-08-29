//! One-shot process shell for one role-scoped durable Proposal Host episode.

use std::{
    fs::OpenOptions,
    io::{Cursor, Read, Write},
    path::Path,
};

use cairn_agent::{
    AdapterVersion, AgentEpisode, AgentEpisodeState, HttpModelTransport,
    MaterializedRequestArtifact, ModelTransport, ModelTransportResponse, NativeProtocolCodec,
    PreparedModelRequest, RecordedExchange, RecordedModelTransport, ResolvedRuntimeModel,
    ResolvedRuntimeModelArtifact, TransportError, recover_agent_episode,
};
use cairn_migration::{ProposalHostRequestV1, ProposalHostTerminalV1, run_proposal_host_episode};
use cairn_protocol::ContentId;
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::Deserialize;

const MAX_REQUEST_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedExchangeWire {
    request_id: ContentId<MaterializedRequestArtifact>,
    response_bytes: Vec<u8>,
}

enum HostTransport {
    Http(HttpModelTransport),
    Recorded(RecordedModelTransport),
}

impl ModelTransport for HostTransport {
    fn dispatch(
        &mut self,
        request: &PreparedModelRequest,
    ) -> Result<ModelTransportResponse, TransportError> {
        match self {
            Self::Http(transport) => transport.dispatch(request),
            Self::Recorded(transport) => transport.dispatch(request),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cairn_observability::init("proposal-host")?;
    let root = std::env::current_dir()?;
    let mut args = std::env::args().skip(1);
    let usage = "usage: cairn-proposal-host <state-root> <resolved-runtime-model-path> [recorded-exchanges-path]";
    let state_root = root.join(args.next().ok_or(usage)?);
    let model_path = root.join(args.next().ok_or(usage)?);
    let recorded_path = args.next().map(|path| root.join(path));
    if args.next().is_some() {
        return Err(usage.into());
    }

    let bytes = std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .bytes()
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.len() > usize::try_from(MAX_REQUEST_BYTES)? {
        return Err("Proposal Host request exceeds the current-V1 byte limit".into());
    }
    let request: ProposalHostRequestV1 = cairn_codec::from_slice(&bytes)?;
    if cairn_codec::to_vec(&request)? != bytes {
        return Err("Proposal Host request is not canonical current-V1 bytes".into());
    }

    let model_bytes = std::fs::read(model_path)?;
    let model: ResolvedRuntimeModel = cairn_codec::from_slice(&model_bytes)?;
    if model.canonical_bytes()? != model_bytes
        || model.content_id()?.to_wire() != request.runtime().model_configuration().to_wire()
        || model.provider() != &request.runtime().selection().provider
        || model.wire_model() != &request.runtime().selection().model
        || model.deployment() != &request.runtime().selection().deployment
        || request.runtime().selection().adapter_version
            != AdapterVersion::new("native-protocol-v1")?
        || request.runtime().max_output_tokens() > model.capabilities().max_output_tokens()
    {
        return Err("resolved runtime model changed the frozen Host invocation".into());
    }

    let state = state_root.join(request.runtime().episode_id().to_string());
    std::fs::create_dir_all(&state)?;
    let content_database = state.join("content.db");
    let event_database = state.join("events.db");
    let cas = state.join("cas");
    let mut content = SqliteContentStore::open(&content_database, &cas)?;
    let mut events = SqliteEventStore::open(&event_database)?;
    let archived = content
        .put::<ResolvedRuntimeModelArtifact>(&mut Cursor::new(&model_bytes))?
        .content_id;
    if archived != model.content_id()? {
        return Err("resolved runtime model identity changed during Host archival".into());
    }
    let terminal_path = state.join("terminal.v1.json");
    if terminal_path.exists() {
        let terminal_bytes = std::fs::read(&terminal_path)?;
        let terminal: ProposalHostTerminalV1 = cairn_codec::from_slice(&terminal_bytes)?;
        if cairn_codec::to_vec(&terminal)? != terminal_bytes {
            return Err(
                "persisted Proposal Host terminal is not canonical current-V1 bytes".into(),
            );
        }
        terminal.validate_against(&request)?;
        validate_durable_episode(&events, &mut content, &request, &terminal)?;
        std::io::stdout().write_all(&terminal_bytes)?;
        return Ok(());
    }
    let codec = NativeProtocolCodec::from_config(model.protocol())?;
    let mut transport = if let Some(path) = recorded_path {
        let exchanges: Vec<RecordedExchangeWire> = cairn_codec::from_slice(&std::fs::read(path)?)?;
        HostTransport::Recorded(RecordedModelTransport::new(exchanges.into_iter().map(
            |exchange| RecordedExchange {
                request_id: exchange.request_id,
                response_bytes: exchange.response_bytes,
                usage: None,
            },
        )))
    } else {
        HostTransport::Http(HttpModelTransport::new(&model, &root)?)
    };
    let terminal = run_proposal_host_episode(
        &mut events,
        &mut content,
        &mut transport,
        codec,
        request.clone(),
    )?;
    terminal.validate_against(&request)?;

    drop(transport);
    drop(events);
    drop(content);
    let mut reopened_content = SqliteContentStore::open(&content_database, &cas)?;
    let reopened_events = SqliteEventStore::open(&event_database)?;
    validate_durable_episode(&reopened_events, &mut reopened_content, &request, &terminal)?;
    let terminal_bytes = cairn_codec::to_vec(&terminal)?;
    persist_terminal_checkpoint(&terminal_path, &terminal_bytes)?;

    std::io::stdout().write_all(&terminal_bytes)?;
    Ok(())
}

fn validate_durable_episode(
    events: &SqliteEventStore,
    content: &mut SqliteContentStore,
    request: &ProposalHostRequestV1,
    terminal: &ProposalHostTerminalV1,
) -> Result<(), Box<dyn std::error::Error>> {
    let recovered = recover_agent_episode(
        events,
        content,
        &AgentEpisode::new(request.runtime().episode_id())?,
    )?;
    let AgentEpisodeState::Completed {
        reason,
        steps_started,
    } = recovered
    else {
        return Err("reopened Proposal Host episode was not durably completed".into());
    };
    if reason != terminal.completion_reason() || steps_started != terminal.steps_started() {
        return Err("reopened Proposal Host terminal projection changed".into());
    }
    Ok(())
}

fn persist_terminal_checkpoint(
    terminal_path: &Path,
    terminal_bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let pending = terminal_path.with_extension("v1.pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|error| format!("Proposal Host terminal requires reconciliation: {error}"))?;
    file.write_all(terminal_bytes)?;
    file.sync_all()?;
    std::fs::rename(&pending, terminal_path)?;
    Ok(())
}
