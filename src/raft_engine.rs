use std::sync::mpsc::{Receiver, TryRecvError, Sender};
use crate::{Update, RaftNode, Proposal, Block, now, RaftMessage};
use std::thread;
use std::time::{Duration, Instant};
use raft::storage::MemStorage;
use raft::RawNode;
use raft::{prelude::*, StateRole};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::collections::VecDeque;
use crate::p2p::network_manager::{NetworkManager, NetworkManagerMessage};
use crate::blockchain::block::BlockType;
use crate::Blockchain;


pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(50);


pub struct RaftEngine {
    pub proposals_global: VecDeque<Proposal>,
    pub conf_change_proposals: VecDeque<Proposal>,
    network_manager_sender: Sender<NetworkManagerMessage>,
    pub raft_engine_client: Sender<RaftNodeMessage>,
    raft_engine_receiver: Receiver<RaftNodeMessage>,
    raft_node_id: u64,
}

impl RaftEngine {

    pub fn new(
        network_manager: Sender<NetworkManagerMessage>,
        raft_node_id: u64
    ) -> Self{
        let (tx, rx) = mpsc::channel();
        RaftEngine {
            proposals_global : VecDeque::<Proposal>::new(),
            conf_change_proposals: VecDeque::<Proposal>::new(),
            network_manager_sender: network_manager,
            raft_engine_client: tx,
            raft_engine_receiver: rx,
            raft_node_id: raft_node_id,
        }
    }

    pub fn start(
        &mut self,
        is_leader: bool,
        peer_list: Vec<u64>,
        mut block_chain: Arc<RwLock<Blockchain>>,
    ){
        let mut t = Instant::now();
        let mut leader_stop_timer = Instant::now();
        let mut new_block_timer = Instant::now();

        let mut raft_node = RaftNode::new(
            self.raft_node_id,
            self.network_manager_sender.clone(),
            peer_list.clone(), block_chain.clone()
        );

        loop {
            // Step raft messages.
            match self.raft_engine_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(update) => {
                    debug!("Update: {:?}", update);
                    self.handle_update( &mut raft_node, update);
                }

            }
            thread::sleep(Duration::from_millis(10));
            let raft = match raft_node.raw_node {
                Some(ref mut r) => r,
                // When Node::raft is `None` it means the the node was not initialized
                _ => continue,
            };

            if t.elapsed() >= RAFT_TICK_TIMEOUT {
                // Tick the raft.
                raft.tick();
                // Reset timer
                t = Instant::now();
            }


            // Let the leader pick pending proposals from the global queue.
            //TODO: Propose new block only when the block is valid
            //TODO: Make sure that new block block will be proposed after previous block was finally committed/rejected by network
            if raft.raft.state == StateRole::Leader {
                // Handle new proposals.
                for p in self.proposals_global.iter_mut().skip_while(|p| p.proposed > 0) {
                    raft_node.propose(p);
                }
                for p in self.conf_change_proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                    raft_node.propose(p);
                }

                //Add new Block
                if new_block_timer.elapsed() >= Duration::from_secs(20) {
                    let mut new_block_id;
                    let new_block;
                    if let Some(last_block) = block_chain.read().expect("BlockChain Lock is poisoned").get_last_block() {
                        new_block_id = last_block.header.block_id + 1;
                        new_block = Block::new(new_block_id, 1,BlockType::Normal,0,"1".to_string());

                    }else {
                        //First block - genesis
                        new_block_id = 1;
                        new_block = Block::genessis(peer_list.clone(), self.raft_node_id)
                    }
                    println!("----------------------");
                    println!("Adding new block - {}",new_block_id);
                    println!("----------------------");
                    let (proposal, rx) = Proposal::new_block(new_block.clone());
                    self.proposals_global.push_back(proposal);
                    raft_node.uncommited_block_queue.insert(new_block_id, new_block.clone());
                    new_block_timer = Instant::now();
                }
            }

            let x = match raft_node.raw_node {
                Some(ref mut r) => r,
                // When Node::raft is `None` it means the the node was not initialized
                _ => continue,
            };


            //If node is follower - reset leader_stop_timer every time
            if x.raft.state == StateRole::Follower{
                leader_stop_timer = Instant::now();
            }
            //if node is Leader for longer then 60 seconds - sleep node threed, new leader should
            //be elected and after wake up this node should catch current log and blockchain state
            if x.raft.state == StateRole::Leader && block_chain.read().expect("BlockChain Lock is poisoned").blocks.len() >3 && leader_stop_timer.elapsed() >= Duration::from_secs(60){
                print!("Leader {:?} is going to sleep for 60 seconds - new election should be held.\n", x.raft.id);
                thread::sleep(Duration::from_secs(30));
                leader_stop_timer = Instant::now();
            }
            raft_node.on_ready();

        }

    }

    pub fn on_block_new(&self, block_chain: Arc<RwLock<Blockchain>>, block: Block) {
        let block_id = block.header.block_id;
        block_chain.write().expect("Blockchain is poisoned").add_block(block);
        println!("Node {} added new block at index {}", self.raft_node_id, block_id);
    }

    fn handle_update(&self, raft_node: &mut RaftNode, update: RaftNodeMessage) -> bool {
        match update {
            RaftNodeMessage::BlockNew(block) => raft_node.on_block_new( block),
            RaftNodeMessage::RaftMessage(message) => raft_node.on_raft_message(&message.content),
            //Update::Shutdown => return false,

            update => warn!("Unhandled update: {:?}", update),
        }
        true
    }

    pub fn propose_to_raft( &mut self, ){

    }
}

#[derive(Debug)]
pub enum RaftNodeMessage{
    RaftMessage(RaftMessage),
    RaftProposal(Proposal),
    BlockNew(Block),
}


//TODO: Remmove, not used after downgrade to raft 0.5.0
fn add_new_node(proposals: &Mutex<VecDeque<Proposal>>, node_id: u64) {
    let mut conf_change = ConfChange::default();
    conf_change.set_node_id(node_id);
    conf_change.set_change_type(ConfChangeType::AddNode);
    let (proposal, rx) = Proposal::conf_change(&conf_change);
    proposals.lock().unwrap().push_back(proposal);
    if rx.recv().unwrap() {
        println!("Node {:?} succesfully added to cluster", node_id);
    }
}

pub fn propose(raft_node: &mut RaftNode, update: RaftNodeMessage){

}









