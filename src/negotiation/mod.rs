//! Negotiation Space核心模块

pub mod crypt;
pub mod message;
pub mod proposal;
pub mod session;
pub mod space;

pub use crypt::{decrypt_content, encrypt_content, generate_key};
pub use message::MessageType;
pub use proposal::{
    BestTermsShared, CreateRfpRequest, Proposal, ProposalDimensions, ProposalResponseAction,
    ProposalStatus, ProposalType, RespondToProposalRequest, RfpContext, ShareBestTermsRequest,
    SubmitProposalRequest,
};
pub use session::SpaceManager;
pub use space::{
    CloseSpaceRequest, CreateSpaceRequest, EncryptedContent, MessageVisibility, SendMessageRequest,
    Space, SpaceMessage, SpaceStatus, SpaceType,
};
