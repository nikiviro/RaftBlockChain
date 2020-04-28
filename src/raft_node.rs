use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};

use prost::Message as ProstMsg;
use protobuf::{self, Message as ProtobufMessage};
use raft::{prelude::*, StateRole};
use raft::eraftpb::ConfState;
use raft::storage::MemStorage;

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;
use crate::proposal::Proposal;
use crate::p2p::network_manager::{NetworkManager, NetworkManagerMessage, SendToRequest, BroadCastRequest, NetworkMessageType, RequestBlockMessage};
use crate::now;
use protobuf::reflect::ProtobufValue;
use rand::prelude::*;
use crate::blockchain::block::{BlockType, ConfiglBlockBody, BlockBody};
use std::time::Instant;

pub struct RaftNode {
    // None if the raft is not initialized.
    pub id: u64,
    pub raw_node: Option<RawNode<MemStorage>>,
    pub network_manager_sender: Sender<NetworkManagerMessage>,
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub leader_state: Option<LeaderState>,
    follower_state: Option<FollowerState>,
}

pub enum LeaderState {
    Building(Instant), // Instant is when the block started being built
    Proposing,
}

enum FollowerState {
    Idle
}

impl RaftNode {
    // Create a raft node with peers from config
    pub fn new(
        id: u64,
        network_manager: Sender<NetworkManagerMessage>,
        genesis_config: ConfiglBlockBody,
        block_chain: Arc<RwLock<Blockchain>>
    ) -> Self {
        //TODO: Load configuration from genesis/configuration block
        let mut rng = rand::thread_rng();
        let election_tick = rng.gen_range(300, 1000);
        println!("Election tick:{}",election_tick);
        let mut cfg = Config {
            election_tick: election_tick,
            heartbeat_tick: 30,
            id: id,
            tag: format!("raft_node{}", id),
            pre_vote: true,
            ..Default::default()
        };


        let mut raft_peers: Vec<Peer> = vec![];
        //Add all electors from genesis file to raft_peers
        for (key, value) in genesis_config.list_of_elector_nodes {
            raft_peers.push(
                Peer{
                    id: key,
                    context: None,
                }
            );
        }
        let storage = MemStorage::default();
        let raft = Some(RawNode::new(&cfg, storage, raft_peers).unwrap());
        RaftNode {
            id,
            raw_node: raft,
            network_manager_sender: network_manager,
            blockchain: block_chain,
            leader_state: None,
            follower_state: Some(FollowerState::Idle)
        }
    }

    // Step a raft message, initialize the raft if need.
    pub fn step(&mut self, msg: Message) {
        if (msg.msg_type != MessageType::MsgHeartbeat && msg.msg_type != MessageType::MsgHeartbeatResponse){
            info!("Received raft message - type: {:?}, from:{:?}, commit:{:?}, entries:{:?}", msg.msg_type, msg.from, msg.commit, msg.entries);
        }
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

    //TODO: Remmove, not used after downgrade to raft 0.5.0
    pub fn initialize_raft_from_message(&mut self, msg: &Message) {
        if !is_initial_msg(msg) {
            return;
        }
        let mut cfg = Config {
            election_tick: 100,
            heartbeat_tick: 3,
            id: msg.get_to(),
            tag: format!("raft_node{}", msg.get_to()),
            pre_vote: true,
            ..Default::default()
        };
        let storage = MemStorage::new();
        self.raw_node = Some(RawNode::new(&cfg, storage, vec![]).unwrap());
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
            //TODO: Add block_hash field to Block struct, so we will dont need to hash block each time we need hash.
            let raft_log_entry: RaftLogEntry = RaftLogEntry::new(block.header.block_id, block.hash());
            //convert Block struct to bytes
            let data = bincode::serialize(&raft_log_entry).unwrap();
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

        let is_leader = raw_node.raft.state == StateRole::Leader;

        //just become a leader
        if is_leader && self.leader_state.is_none(){
            self.leader_state = Some(LeaderState::Building(Instant::now()));
        }

        if !is_leader && self.leader_state.is_some(){
            self.leader_state = None;
            self.follower_state = Some(FollowerState::Idle);
        }

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
            //info!("Sending message:{:?}",msg);
            let to = msg.get_to();
            let message_to_send = NetworkMessageType::RaftMessage(RaftMessage::new(msg));
            self.network_manager_sender.send(NetworkManagerMessage::SendToRequest(SendToRequest::new(to, message_to_send)));
        }

        let mut block_chain = self.blockchain.write().expect("BlockChain Lock is poisoned");

        // Apply all committed proposals.
        if let Some(committed_entries) = ready.committed_entries.take() {
            for entry in &committed_entries {
                if entry.get_data().is_empty() {
                    // From new elected leaders.
                    continue;
                }
                if let EntryType::EntryConfChange = entry.get_entry_type() {
                    //TODO: Make sure that there is only on conf change proposal at time
                    // Handle conf change messages
                    let mut cc = ConfChange::default();
                    cc.merge_from_bytes(&entry.data).unwrap();
                    let node_id = cc.get_node_id();
                    match cc.get_change_type() {
                        ConfChangeType::AddNode => {
                            if (entry.get_term() != 1){
                                raw_node.raft.add_node(node_id).unwrap();
                            }
                        },
                        ConfChangeType::RemoveNode => raw_node.raft.remove_node(node_id).unwrap(),
                        ConfChangeType::AddLearnerNode => raw_node.raft.add_learner(node_id).unwrap(),
                        ConfChangeType::BeginMembershipChange
                        | ConfChangeType::FinalizeMembershipChange => unimplemented!(),
                    }
                    let cs = ConfState::from(raw_node.raft.prs().configuration().clone());
                    store.wl().set_conf_state(cs, None);

                }  else {
                    // For normal proposals, extract block from message
                    // If node is leader and blockid was commited:
                    //  - insert block into the blockchain
                    //  - announce new block to network

                    let raft_log_entry :RaftLogEntry = bincode::deserialize(&entry.get_data()).unwrap();

                    if is_leader{
                        //TODO: Do not create new block, but load it from uncommited block que
                        if block_chain.uncommited_block_queue.contains_key(&raft_log_entry.block_hash){
                            //TODO: make find/remove methods for uncomiited block que in blockchain.rs
                            match block_chain.remove_from_uncommitted_block_que(&raft_log_entry.block_hash) {
                                Some(block) => {
                                    block_chain.add_block(block.clone());
                                    info!("[BLOCK COMMITTED - {}] Leader added new block: {:?}", block.hash(), block);

                                    let message_to_send = NetworkMessageType::BlockNew(block_chain.get_last_block().unwrap());
                                    self.network_manager_sender.send(NetworkManagerMessage::BroadCastRequest(BroadCastRequest::new(message_to_send)));

                                    self.leader_state = Some(LeaderState::Building(Instant::now()));
                                },
                                None => panic!("Raft leader committed block which is not in its uncommitted block que - hash:{}!",&raft_log_entry.block_hash)
                            }
                        }
                        else{
                            //Leader must have committed block in uncommitted block que, because block was created by this leader
                            panic!("Raft leader committed block which is not in its uncommitted block que - hash:{}!",&raft_log_entry.block_hash)
                        }
                    }
                    else{
                        if block_chain.uncommited_block_queue.contains_key(&raft_log_entry.block_hash){
                            match block_chain.remove_from_uncommitted_block_que(&raft_log_entry.block_hash) {
                                Some(block) => {
                                    block_chain.add_block(block.clone());
                                    info!("[BLOCK COMMITTED - {}] Follower added new block: {:?}", block.hash(), block);
                                    let message_to_send = NetworkMessageType::BlockNew(block_chain.get_last_block().unwrap());
                                    self.network_manager_sender.send(NetworkManagerMessage::BroadCastRequest(BroadCastRequest::new(message_to_send)));
                                },
                                None => panic!("Raft follower committed block which is not in its uncommitted block que!")
                            }
                        }
                        else{
                            info!("[DONT HAVE COMMITTED BLOCK - {}] Raft follower committed block which is not in its uncommitted block que!", &raft_log_entry.block_hash);
                        }
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
            if let Some(hs) = ready.hs() {
                // Raft HardState changed, and we need to persist it.
                raw_node.mut_store().wl().set_hardstate(hs.clone());
            }
//            let  leader_id = raw_node.raft.leader_id;
//            println!("Current leader : {} ",leader_id);
        }
        // Call `RawNode::advance` interface to update position flags in the raft.
        raw_node.advance(ready);
    }

    pub fn on_raft_message(&mut self, message: &[u8]){
        let raft_message = protobuf::parse_from_bytes::<Message>(message).unwrap();

        let raw_node = match self.raw_node {
            Some(ref mut r) => r,
            // When Node::raft is `None` it means the the node was not initialized
            _ => panic!("Raft is not innitialized"),
        };

        let mut should_be_processed = true;
        //If message is block append request - first validate block
        if raw_node.raft.state == StateRole::Follower
            && raft_message.msg_type == MessageType::MsgAppend
            && raft_message.entries.len() > 0 && !raft_message.entries[0].get_data().is_empty()
            && raft_message.entries[0].get_entry_type() == EntryType::EntryNormal
        {
            let raft_log_entry :RaftLogEntry = bincode::deserialize(&raft_message.entries[0].get_data()).unwrap();
            should_be_processed = self.handle_block_append_request(raft_log_entry);
        }

        if should_be_processed{
            self.step(raft_message);
        }
    }

    pub fn handle_block_append_request(&mut self,  raft_log_entry: RaftLogEntry) -> bool{


        if self.blockchain.read().expect("BlockChain Lock is poisoned").uncommited_block_queue.contains_key(&raft_log_entry.block_hash)
        {
                debug!("Raft block append accepted - [HAVE BLOCK]");
                true
        }
        else{
            //Request block from other peers
            let message_to_send = NetworkMessageType::RequestBlock(RequestBlockMessage::new(self.id,raft_log_entry.block_id, raft_log_entry.block_hash));
            self.network_manager_sender.send(NetworkManagerMessage::BroadCastRequest(BroadCastRequest::new(message_to_send)));
            debug!("Raft block append denied - [DONT HAVE BLOCK]");
            false
        }
    }

    pub fn on_block_new(&mut self, block: Block) {
    }
}
#[derive(Debug,Serialize, Deserialize)]
pub enum Update {
    BlockNew(Block),
    RaftMessage(RaftMessage)
}

#[derive(Debug,Serialize, Deserialize, Clone)]
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

#[derive(Debug,Serialize, Deserialize)]
pub struct RaftLogEntry{
    pub block_id: u64,
    pub block_hash: String
}

impl RaftLogEntry{
    pub fn new( block_id: u64, block_hash: String) -> Self{
        RaftLogEntry{
            block_id,
            block_hash
        }
    }
}

//TODO: Remmove, not used after downgrade to raft 0.5.0
// The message can be used to initialize a raft node or not.
fn is_initial_msg(msg: &Message) -> bool {
    let msg_type = msg.get_msg_type();
    msg_type == MessageType::MsgRequestVote
        || msg_type == MessageType::MsgRequestPreVote
        || (msg_type == MessageType::MsgHeartbeat && msg.get_commit() == 0)
}