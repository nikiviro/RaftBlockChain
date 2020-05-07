use std::sync::mpsc::{self, Receiver, SyncSender};

use raft::{prelude::*};

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;

#[derive(Debug)]
pub struct Proposal {
    pub block: Option<Block>, // block id of block that should be proposed to nodes
    pub conf_change: Option<ConfChange>, // conf change.
    pub proposed: u64,
    pub propose_success: SyncSender<bool>,
}

impl Proposal {
    pub fn conf_change(cc: &ConfChange) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            block: None,
            conf_change: Some(cc.clone()),
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }

    pub fn new_block(block: Block) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            block: Some(block),
            conf_change: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }
}