//! Gaggle - Agent-to-Agent 商业平台
//!
//! MVP实现：消费者Agent与服务商Agent的接入、Negotiation Space拉起、多轮谈判

pub mod agents;
pub mod api;
pub mod config;
pub mod discovery;
pub mod error;
pub mod execution;
pub mod marketplace;
pub mod negotiation;
pub mod reputation;
#[cfg(feature = "solana")]
pub mod solana;
pub mod templates;
pub mod users;

pub use agents::registry::AgentRegistry;
pub use config::Config;
pub use discovery::DiscoveryStore;
pub use error::GaggleError;
pub use execution::ExecutionStore;
pub use marketplace::MarketplaceStore;
pub use negotiation::{
    BestTermsShared, Coalition, CoalitionStatus, CreateCoalitionRequest, CreateDelegationRequest,
    CreateRecruitmentRequest, CreateRfpRequest, CreateSubSpaceRequest, Delegation, DelegationScope,
    DelegationStatus, JoinPolicy, LockCondition, MessageVisibility, PersistedTransition, Proposal,
    ProposalDimensions, ProposalResponseAction, ProposalStatus, ProposalType, RecruitmentRequest,
    RecruitmentStatus, RevealMode, RespondToProposalRequest, RfpContext, RoleConfig, RoundConfig,
    RuleTransition, RuleTrigger, ShareBestTermsRequest, Space, SpaceRules, SpaceRulesOverrides,
    SpaceStatus, SpaceType, SubSpace, SubmitProposalRequest, TransitionHistory,
    UpdateStanceRequest, VisibilityRule, VisibilityRules,
};
pub use reputation::ReputationStore;
pub use users::UserStore;
