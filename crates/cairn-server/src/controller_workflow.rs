//! Readable Controller workflow skeleton and its strongly typed stage ports.
//!
//! The orchestration function deliberately contains only the product stages. A concrete stage
//! implementation is added only when that stage has a real artifact and authority consumer; an
//! absent implementation therefore cannot be mistaken for a successful workflow transition.

use std::future::Future;

/// Domain ports required to execute one complete CUDA-to-Ascend-C Controller workflow.
///
/// Associated artifact types preserve the semantic boundary between proposals, admitted
/// authority, observations, and terminal outcomes. There are intentionally no default stage
/// implementations: an unimplemented stage remains a compile-time integration gap.
pub trait ControllerWorkflowStages: Send {
    type Error: Send;
    type Request: Send;
    type FrozenRequest: Send + Sync;
    type SirProposal: Send + Sync;
    type AdmittedIntent: Send + Sync;
    type OracleBlueProposal: Send + Sync;
    type OracleRedProposal: Send + Sync;
    type AdmittedOracle: Send + Sync;
    type CandidateProposal: Send + Sync;
    type WorkerObservations: Send + Sync;
    type AdmittedCandidate: Send + Sync;
    type TerminalOutcome: Send;

    fn freeze_controller_request(
        &mut self,
        request: Self::Request,
    ) -> impl Future<Output = Result<Self::FrozenRequest, Self::Error>> + Send;

    fn run_sir_proposal_loop(
        &mut self,
        frozen: &Self::FrozenRequest,
    ) -> impl Future<Output = Result<Self::SirProposal, Self::Error>> + Send;

    fn run_intent_admission_gate(
        &mut self,
        frozen: &Self::FrozenRequest,
        proposal: Self::SirProposal,
    ) -> impl Future<Output = Result<Self::AdmittedIntent, Self::Error>> + Send;

    fn run_oracle_blue_proposal_loop(
        &mut self,
        frozen: &Self::FrozenRequest,
        intent: &Self::AdmittedIntent,
    ) -> impl Future<Output = Result<Self::OracleBlueProposal, Self::Error>> + Send;

    fn run_oracle_red_proposal_loop(
        &mut self,
        frozen: &Self::FrozenRequest,
        intent: &Self::AdmittedIntent,
        blue: &Self::OracleBlueProposal,
    ) -> impl Future<Output = Result<Self::OracleRedProposal, Self::Error>> + Send;

    fn run_oracle_admission_gate(
        &mut self,
        frozen: &Self::FrozenRequest,
        intent: &Self::AdmittedIntent,
        blue: Self::OracleBlueProposal,
        red: Self::OracleRedProposal,
    ) -> impl Future<Output = Result<Self::AdmittedOracle, Self::Error>> + Send;

    fn run_candidate_proposal_loop(
        &mut self,
        frozen: &Self::FrozenRequest,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
    ) -> impl Future<Output = Result<Self::CandidateProposal, Self::Error>> + Send;

    fn collect_worker_observations(
        &mut self,
        frozen: &Self::FrozenRequest,
        candidate: &Self::CandidateProposal,
    ) -> impl Future<Output = Result<Self::WorkerObservations, Self::Error>> + Send;

    fn run_candidate_admission_gate(
        &mut self,
        frozen: &Self::FrozenRequest,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: Self::CandidateProposal,
        observations: Self::WorkerObservations,
    ) -> impl Future<Output = Result<Self::AdmittedCandidate, Self::Error>> + Send;

    fn save_terminal_outcome(
        &mut self,
        frozen: Self::FrozenRequest,
        intent: Self::AdmittedIntent,
        oracle: Self::AdmittedOracle,
        candidate: Self::AdmittedCandidate,
    ) -> impl Future<Output = Result<Self::TerminalOutcome, Self::Error>> + Send;
}

/// Executes the complete Controller business workflow through explicit typed stage ports.
///
/// # Errors
///
/// Returns the concrete stage implementation's error without skipping or fabricating any later
/// artifact.
pub async fn run_controller_workflow<S: ControllerWorkflowStages>(
    stages: &mut S,
    request: S::Request,
) -> Result<S::TerminalOutcome, S::Error> {
    let frozen = stages.freeze_controller_request(request).await?;
    let sir = stages.run_sir_proposal_loop(&frozen).await?;
    let intent = stages.run_intent_admission_gate(&frozen, sir).await?;
    let blue = stages
        .run_oracle_blue_proposal_loop(&frozen, &intent)
        .await?;
    let red = stages
        .run_oracle_red_proposal_loop(&frozen, &intent, &blue)
        .await?;
    let oracle = stages
        .run_oracle_admission_gate(&frozen, &intent, blue, red)
        .await?;
    let candidate = stages
        .run_candidate_proposal_loop(&frozen, &intent, &oracle)
        .await?;
    let observations = stages
        .collect_worker_observations(&frozen, &candidate)
        .await?;
    let candidate = stages
        .run_candidate_admission_gate(&frozen, &intent, &oracle, candidate, observations)
        .await?;
    stages
        .save_terminal_outcome(frozen, intent, oracle, candidate)
        .await
}

#[cfg(test)]
mod tests {
    use std::future::ready;

    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum RecordedStage {
        Freeze,
        Sir,
        IntentAdmission,
        OracleBlue,
        OracleRed,
        OracleAdmission,
        Candidate,
        WorkerObservations,
        CandidateAdmission,
        Terminal,
    }

    struct Request;
    struct FrozenRequest;
    struct SirProposal;
    struct AdmittedIntent;
    struct OracleBlueProposal;
    struct OracleRedProposal;
    struct AdmittedOracle;
    struct CandidateProposal;
    struct WorkerObservations;
    struct AdmittedCandidate;
    #[derive(Debug)]
    struct TerminalOutcome;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct StageFailure(RecordedStage);

    #[derive(Default)]
    struct RecordedStages {
        trace: Vec<RecordedStage>,
        fail_at: Option<RecordedStage>,
    }

    impl RecordedStages {
        fn record(&mut self, stage: RecordedStage) -> Result<(), StageFailure> {
            self.trace.push(stage);
            if self.fail_at == Some(stage) {
                Err(StageFailure(stage))
            } else {
                Ok(())
            }
        }
    }

    impl ControllerWorkflowStages for RecordedStages {
        type Error = StageFailure;
        type Request = Request;
        type FrozenRequest = FrozenRequest;
        type SirProposal = SirProposal;
        type AdmittedIntent = AdmittedIntent;
        type OracleBlueProposal = OracleBlueProposal;
        type OracleRedProposal = OracleRedProposal;
        type AdmittedOracle = AdmittedOracle;
        type CandidateProposal = CandidateProposal;
        type WorkerObservations = WorkerObservations;
        type AdmittedCandidate = AdmittedCandidate;
        type TerminalOutcome = TerminalOutcome;

        fn freeze_controller_request(
            &mut self,
            _request: Self::Request,
        ) -> impl Future<Output = Result<Self::FrozenRequest, Self::Error>> + Send {
            ready(self.record(RecordedStage::Freeze).map(|()| FrozenRequest))
        }

        fn run_sir_proposal_loop(
            &mut self,
            _frozen: &Self::FrozenRequest,
        ) -> impl Future<Output = Result<Self::SirProposal, Self::Error>> + Send {
            ready(self.record(RecordedStage::Sir).map(|()| SirProposal))
        }

        fn run_intent_admission_gate(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _proposal: Self::SirProposal,
        ) -> impl Future<Output = Result<Self::AdmittedIntent, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::IntentAdmission)
                    .map(|()| AdmittedIntent),
            )
        }

        fn run_oracle_blue_proposal_loop(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _intent: &Self::AdmittedIntent,
        ) -> impl Future<Output = Result<Self::OracleBlueProposal, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::OracleBlue)
                    .map(|()| OracleBlueProposal),
            )
        }

        fn run_oracle_red_proposal_loop(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _intent: &Self::AdmittedIntent,
            _blue: &Self::OracleBlueProposal,
        ) -> impl Future<Output = Result<Self::OracleRedProposal, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::OracleRed)
                    .map(|()| OracleRedProposal),
            )
        }

        fn run_oracle_admission_gate(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _intent: &Self::AdmittedIntent,
            _blue: Self::OracleBlueProposal,
            _red: Self::OracleRedProposal,
        ) -> impl Future<Output = Result<Self::AdmittedOracle, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::OracleAdmission)
                    .map(|()| AdmittedOracle),
            )
        }

        fn run_candidate_proposal_loop(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _intent: &Self::AdmittedIntent,
            _oracle: &Self::AdmittedOracle,
        ) -> impl Future<Output = Result<Self::CandidateProposal, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::Candidate)
                    .map(|()| CandidateProposal),
            )
        }

        fn collect_worker_observations(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _candidate: &Self::CandidateProposal,
        ) -> impl Future<Output = Result<Self::WorkerObservations, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::WorkerObservations)
                    .map(|()| WorkerObservations),
            )
        }

        fn run_candidate_admission_gate(
            &mut self,
            _frozen: &Self::FrozenRequest,
            _intent: &Self::AdmittedIntent,
            _oracle: &Self::AdmittedOracle,
            _candidate: Self::CandidateProposal,
            _observations: Self::WorkerObservations,
        ) -> impl Future<Output = Result<Self::AdmittedCandidate, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::CandidateAdmission)
                    .map(|()| AdmittedCandidate),
            )
        }

        fn save_terminal_outcome(
            &mut self,
            _frozen: Self::FrozenRequest,
            _intent: Self::AdmittedIntent,
            _oracle: Self::AdmittedOracle,
            _candidate: Self::AdmittedCandidate,
        ) -> impl Future<Output = Result<Self::TerminalOutcome, Self::Error>> + Send {
            ready(
                self.record(RecordedStage::Terminal)
                    .map(|()| TerminalOutcome),
            )
        }
    }

    #[tokio::test]
    async fn business_skeleton_exposes_the_complete_controller_stage_order() {
        let mut stages = RecordedStages::default();

        let _ = run_controller_workflow(&mut stages, Request)
            .await
            .expect("recorded skeleton");

        assert_eq!(
            stages.trace,
            vec![
                RecordedStage::Freeze,
                RecordedStage::Sir,
                RecordedStage::IntentAdmission,
                RecordedStage::OracleBlue,
                RecordedStage::OracleRed,
                RecordedStage::OracleAdmission,
                RecordedStage::Candidate,
                RecordedStage::WorkerObservations,
                RecordedStage::CandidateAdmission,
                RecordedStage::Terminal,
            ]
        );
    }

    #[tokio::test]
    async fn unavailable_stage_stops_before_any_downstream_authority() {
        let mut stages = RecordedStages {
            fail_at: Some(RecordedStage::OracleBlue),
            ..RecordedStages::default()
        };

        let error = run_controller_workflow(&mut stages, Request)
            .await
            .expect_err("unimplemented Oracle Blue stage must fail closed");

        assert_eq!(error, StageFailure(RecordedStage::OracleBlue));
        assert_eq!(
            stages.trace,
            vec![
                RecordedStage::Freeze,
                RecordedStage::Sir,
                RecordedStage::IntentAdmission,
                RecordedStage::OracleBlue,
            ]
        );
    }
}
