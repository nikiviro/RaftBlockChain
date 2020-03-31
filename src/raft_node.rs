use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};

use prost::Message as ProstMsg;
use protobuf::{self, Message as ProtobufMessage};
use raft::{prelude::*, StateRole};
use raft::eraftpb::ConfState;
use raft::storage::MemStorage;

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;
use crate::proposal::Proposal;
use crate::p2p::network_manager::{NetworkManager, NetworkManagerMessage, SendToRequest, BroadCastRequest};
use crate::now;

pub struct RaftNode {
    // None if the raft is not initialized.
    pub id: u64,
    pub raw_node: Option<RawNode<MemStorage>>,
    pub network_manager_sender: Sender<NetworkManagerMessage>,
    pub blockchain: Blockchain,
    pub is_node_without_raft: bool,
    pub is_changing_config: bool //TODO: Obsolete, remove!
}

impl RaftNode {
    // Create a raft leader only with itself in its configuration.
    pub fn create_raft_leader(
        id: u64,
        network_manager: Sender<NetworkManagerMessage>,
//        peers_list: Vec<u64>,
    ) -> Self {
        //TODO: Load configuration from genesis/configuration block
        let mut cfg = Config {
            election_tick: 10,
            heartbeat_tick: 3,
            id: id,
            tag: format!("raft_node{}", id),
            ..Default::default()
        };

        let storage = MemStorage::new_with_conf_state(ConfState::from((vec![id], vec![])));
        let raft = Some(RawNode::new(&cfg, storage).unwrap());
        RaftNode {
            id,
            raw_node: raft,
            network_manager_sender: network_manager,
            blockchain: Blockchain::new(),
            is_node_without_raft: false,
            is_changing_config: false
        }
    }

    // Create a raft follower.
    pub fn create_raft_follower(
        id: u64,
        network_manager: Sender<NetworkManagerMessage>,
        is_node_without_raft: bool,
    ) -> Self {
        //TODO: Load configuration from genesis/configuration block
        RaftNode {
            id,
            raw_node: None,
            network_manager_sender: network_manager,
            blockchain: Blockchain::new(),
            is_node_without_raft: is_node_without_raft,
            is_changing_config: false
        }
    }

    // Step a raft message, initialize the raft if need.
    pub fn step(&mut self, msg: Message) {
        if self.raw_node.is_none() {
            if is_initial_msg(&msg) {
                self.initialize_raft_from_message(&msg);
            } else {
                return;
            }
        }
        let raft = self.raw_node.as_mut().unwrap();
        let _ = raft.step(msg);
    }

    pub fn initialize_raft_from_message(&mut self, msg: &Message) {
        if !is_initial_msg(msg) {
            return;
        }
        let mut cfg = Config {
            election_tick: 100,
            heartbeat_tick: 3,
            id: msg.get_to(),
            tag: format!("raft_node{}", msg.get_to()),
            ..Default::default()
        };
        let storage = MemStorage::new();
        self.raw_node = Some(RawNode::new(&cfg, storage).unwrap());
    }

    pub fn start_node(&mut self) {

    }

    pub fn propose(&mut self, proposal: &mut Proposal) {
        let raw_node = match self.raw_node {
            Some(ref mut r) => r,
            // When Node::raft is `None` it means the the node was not initialized
            _ => panic!("Raft is not innitialized"),
        };

        let last_index1 = raw_node.raft.raft_log.last_index() + 1;
        if let Some( ref block) = proposal.block {
            //convert Block struct to bytes
            let data = bincode::serialize(&block.block_id).unwrap();
            //let new_block_id = block.block_id;
            let _ = raw_node.propose(vec![], data);
        } else if let Some(ref cc) = proposal.conf_change {
            let _ = raw_node.propose_conf_change(vec![], cc.clone());
        }

        let last_index2 = raw_node.raft.raft_log.last_index() + 1;
        if last_index2 == last_index1 {
            // Propose failed, don't forget to respond to the client.
            //proposal.propose_success.send(false).unwrap();
        } else {
            proposal.proposed = last_index1;
        }
    }

    pub fn on_ready(
        &mut self,
        conf_change_proposals: &Mutex<VecDeque<Proposal>>,
    ) {
        let raw_node = match self.raw_node {
            Some(ref mut r) => r,
            // When Node::raft is `None` it means the the node was not initialized
            _ => panic!("Raft is not innitialized"),
        };

        if !raw_node.has_ready() {
            return;
        }
        let store = raw_node.raft.raft_log.store.clone();

        // Get the `Ready` with `RawNode::ready` interface.
        let mut ready = raw_node.ready();

        // Persistent raft logs. It's necessary because in `RawNode::advance` we stabilize
        // raft logs to the latest position.
        if let Err(e) = store.wl().append(ready.entries()) {
            error!("persist raft log fail: {:?}, need to retry or panic", e);
            return;
        }

        // Apply the snapshot. It's necessary because in `RawNode::advance` we stabilize the snapshot.
        if *ready.snapshot() != Snapshot::default() {
            let s = ready.snapshot().clone();
            if let Err(e) = store.wl().apply_snapshot(s) {
                error!("apply snapshot fail: {:?}, need to retry or panic", e);
                return;
            }
        }

        // Send out the messages come from the node.
        for msg in ready.messages.drain(..) {
            //print!("Sending message:{:?}",msg);
            let to = msg.get_to();
            let message_to_send = Update::RaftMessage(RaftMessage::new(msg));
            let data = bincode::serialize(&message_to_send).expect("Error while serializing Update RaftMessage");
            //let data = msg.write_to_bytes().unwrap();
            self.network_manager_sender.send(NetworkManagerMessage::SendToRequest(SendToRequest::new(to, data)));
        }

        // Apply all committed proposals.
        if let Some(committed_entries) = ready.committed_entries.take() {
            for entry in &committed_entries {
                if entry.get_data().is_empty() {
                    // From new elected leaders.
                    continue;
                }
                if let EntryType::EntryConfChange = entry.get_entry_type() {
                    // Handle conf change messages
                    let mut cc = ConfChange::default();
                    cc.merge_from_bytes(&entry.data).unwrap();
                    let node_id = cc.get_node_id();
                    match cc.get_change_type() {
                        ConfChangeType::AddNode => raw_node.raft.add_node(node_id).unwrap(),
                        ConfChangeType::AddNode => raw_node.raft.add_node(node_id).unwrap(),
                        ConfChangeType::RemoveNode => raw_node.raft.remove_node(node_id).unwrap(),
                        ConfChangeType::AddLearnerNode => raw_node.raft.add_learner(node_id).unwrap(),
                        ConfChangeType::BeginMembershipChange
                        | ConfChangeType::FinalizeMembershipChange => unimplemented!(),
                    }
                    let cs = ConfState::from(raw_node.raft.prs().configuration().clone());
                    store.wl().set_conf_state(cs, None);

                    if raw_node.raft.state == StateRole::Leader {
                        // The leader should response to the clients, tell them if their proposals
                        // succeeded or not.
                        let conf_change_proposals = conf_change_proposals.lock().unwrap().pop_front().unwrap();
                        conf_change_proposals.propose_success.send(true).unwrap();
                    }


                }  else {
                    // For normal proposals, extract block from message
                    // If node is leader and blockid was commited:
                    //  - insert block into the blockchain
                    //  - announce new block to network

                    let block_id: u64 = bincode::deserialize(&entry.get_data()).unwrap();
                    if raw_node.raft.state == StateRole::Leader{
                        let block = block::Block::new(block_id, 0, 0, 0, "".to_string(), now(), vec![0], vec![0], vec![0]);
                        self.blockchain.add_block(block);
                        println!("Leader added new block: {:?}", self.blockchain.get_last_block());
                        println!("Node {} - Leader added new block at index {}",raw_node.raft.id, block_id);

                        let message_to_send = Update::BlockNew(self.blockchain.get_last_block().unwrap());
                        let data = bincode::serialize(&message_to_send).expect("Error while serializing Update (New block) RaftMessage");

                        self.network_manager_sender.send(NetworkManagerMessage::BroadCastRequest(BroadCastRequest::new(data)));

                    }
//                    //let block_index = block.block_id;
//                    let node_role;
//                    if raw_node.raft.state == StateRole::Leader{
//                        node_role = String::from("Leader");
//                    }else{
//                        node_role = String::from("Follower");
//                    }
                }
            }
            if let Some(last_committed) = committed_entries.last() {
                let mut s = store.wl();
                s.mut_hard_state().set_commit(last_committed.get_index());
                s.mut_hard_state().set_term(last_committed.get_term());
            }
//            let  leader_id = raw_node.raft.leader_id;
//            println!("Current leader : {} ",leader_id);
        }
        // Call `RawNode::advance` interface to update position flags in the raft.
        raw_node.advance(ready);
    }

    pub fn on_raft_message(&mut self, message: &[u8]){
        let raft_message = protobuf::parse_from_bytes::<Message>(message).unwrap();
        self.step(raft_message);
    }

    pub fn on_block_new(&mut self, block: Block) {
        let block_id = block.block_id;
        self.blockchain.add_block(block);
        println!("Node {} added new block at index {}", self.id, block_id);
    }
}
#[derive(Debug,Serialize, Deserialize)]
pub enum Update {
    BlockNew(Block),
    RaftMessage(RaftMessage)
}

#[derive(Debug,Serialize, Deserialize)]
pub struct RaftMessage{
    pub content: Vec<u8>,
}

impl RaftMessage{
    pub fn new(
        message: Message,
    ) -> Self {
        let content =  message.write_to_bytes().expect("Error while serializing raft message!");

        RaftMessage{
            content,
        }
    }
}
// The message can be used to initialize a raft node or not.
fn is_initial_msg(msg: &Message) -> bool {
    let msg_type = msg.get_msg_type();
    msg_type == MessageType::MsgRequestVote
        || msg_type == MessageType::MsgRequestPreVote
        || (msg_type == MessageType::MsgHeartbeat && msg.get_commit() == 0)
}