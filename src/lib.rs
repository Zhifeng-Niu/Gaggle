//! Gaggle - Agent-to-Agent 商业平台
//!
//! MVP实现：消费者Agent与服务商Agent的接入、Negotiation Space拉起、多轮谈判

pub mod agents;
pub mod api;
pub mod config;
pub mod discovery;
pub mod error;
pub mod negotiation;
pub mod reputation;
pub mod solana;
pub mod users;

pub use agents::registry::AgentRegistry;
pub use config::Config;
pub use discovery::DiscoveryStore;
pub use error::GaggleError;
pub use negotiation::{
    BestTermsShared, CreateRfpRequest, MessageVisibility, Proposal, ProposalDimensions,
    ProposalResponseAction, ProposalStatus, ProposalType, RespondToProposalRequest, RfpContext,
    ShareBestTermsRequest, Space, SpaceStatus, SpaceType, SubmitProposalRequest,
};
pub use reputation::ReputationStore;
pub use users::UserStore;
