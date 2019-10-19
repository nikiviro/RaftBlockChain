use raft::{prelude::*, StateRole};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};

pub use crate::blockchain::block::Block;
pub use crate::blockchain::*;

pub struct Proposal {
    pub normal: Option<Block>, // key is an u16 integer, and value is a string.
    pub conf_change: Option<ConfChange>, // conf change.
    pub proposed: u64,
    pub propose_success: SyncSender<bool>,
}

impl Proposal {
    pub fn conf_change(cc: &ConfChange) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            normal: None,
            conf_change: Some(cc.clone()),
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }

    pub fn normal(block: Block) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            normal: Some(block),
            conf_change: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }
}