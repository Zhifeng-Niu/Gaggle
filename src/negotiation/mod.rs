//! Negotiation Space核心模块

pub mod coalition;
pub mod crypt;
pub mod delegation;
pub mod message;
pub mod proposal;
pub mod recruitment;
pub mod rules;
pub mod session;
pub mod space;
pub mod subspace;
pub mod visibility;

pub use crypt::{decrypt_content, encrypt_content, generate_key};
pub use message::MessageType;
pub use proposal::{
    BestTermsShared, CreateRfpRequest, DimensionScores, EvaluateRequest, EvaluateResponse,
    EvaluationWeights, Proposal, ProposalDimensions, ProposalResponseAction, ProposalScore,
    ProposalStatus, ProposalType, RespondToProposalRequest, RfpContext, RoundInfo, RoundStatus,
    ShareBestTermsRequest, SubmitProposalRequest,
};
pub use rules::{
    JoinPolicy, LockCondition, RevealMode, RoleConfig, RoundConfig, RuleTransition, RuleTrigger,
    SpaceRules, SpaceRulesOverrides, VisibilityRule, VisibilityRules,
};
pub use session::SpaceManager;
pub use visibility::VisibilityEngine;
pub use space::{
    CloseSpaceRequest, CreateSpaceRequest, EncryptedContent, MessageVisibility, SendMessageRequest,
    Space, SpaceMessage, SpaceStatus, SpaceType,
};
pub use subspace::{CreateSubSpaceRequest, SubSpace};
pub use coalition::{Coalition, CoalitionStatus, CreateCoalitionRequest, UpdateStanceRequest};
pub use delegation::{CreateDelegationRequest, Delegation, DelegationScope, DelegationStatus};
pub use recruitment::{CreateRecruitmentRequest, RecruitmentRequest, RecruitmentStatus};
