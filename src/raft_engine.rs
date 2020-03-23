use std::sync::mpsc::{Receiver, TryRecvError, Sender};
use crate::{Update, RaftNode, Proposal, Block, now};
use std::thread;
use std::time::{Duration, Instant};
use raft::storage::MemStorage;
use raft::RawNode;
use raft::{prelude::*, StateRole};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::collections::VecDeque;
use crate::p2p::network_manager::{NetworkManager, NetworkManagerMessage};

pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(100);


pub struct RaftEngine {
    pub proposals_global: Arc<Mutex<VecDeque<Proposal>>>,
    pub conf_change_proposals_global: Arc<Mutex<VecDeque<Proposal>>>,
    network_manager_sender: Sender<NetworkManagerMessage>,
    pub raft_engine_client: Sender<Update>,
    raft_engine_receiver: Receiver<Update>
}

impl RaftEngine {

    pub fn new(
        network_manager: Sender<NetworkManagerMessage>
    ) -> Self{
        let (tx, rx) = mpsc::channel();
        RaftEngine {
            proposals_global : Arc::new(Mutex::new(VecDeque::<Proposal>::new())),
            conf_change_proposals_global: Arc::new(Mutex::new(VecDeque::<Proposal>::new())),
            network_manager_sender: network_manager,
            raft_engine_client: tx,
            raft_engine_receiver: rx
        }
    }

    pub fn start(
        &mut self,
        raft_node_id: u64,
        is_leader: bool,
        peer_list: Vec<u64>
    ){

        let proposals = Arc::clone(&self.proposals_global);
        let conf_change_proposals = Arc::clone(&self.conf_change_proposals_global);

        let mut t = Instant::now();
        let mut leader_stop_timer = Instant::now();
        let mut new_block_timer = Instant::now();

        let mut proposal_responses = Vec::new();

        let mut raft_node = match is_leader {
            // Create node 1 as leader
            true => RaftNode::create_raft_leader(
                raft_node_id,
                self.network_manager_sender.clone()),
            // Other nodes are followers.
            _ => RaftNode::create_raft_follower(
                raft_node_id,
                self.network_manager_sender.clone(),
                true)
        };

        loop {
            // Step raft messages.
            match self.raft_engine_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(update) => {
                    debug!("Update: {:?}", update);
                    handle_update( &mut raft_node,update);
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
                let mut proposals = proposals.lock().unwrap();
                for p in proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                    raft_node.propose(p);
                }
                let mut conf_change_proposals = conf_change_proposals.lock().unwrap();
                for p in conf_change_proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                    raft_node.propose(p);
                }

                //Add new Block
                if new_block_timer.elapsed() >= Duration::from_secs(20) {
                    let mut new_block_id;

                    if let Some(last_block) = raft_node.blockchain.get_last_block() {
                        new_block_id = last_block.block_id + 1;
                    }else {
                        //First block - genesis
                        new_block_id = 1;
                    }
                    println!("----------------------");
                    println!("Adding new block - {}",new_block_id);
                    println!("----------------------");
                    let new_block = Block::new(new_block_id, 1,1,0,"1".to_string(),now(), vec![0; 32], vec![0; 32], vec![0; 64]);
                    let (proposal, rx) = Proposal::new_block(new_block);
                    proposal_responses.push(rx);
                    proposals.push_back(proposal);
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
            if x.raft.state == StateRole::Leader && raft_node.blockchain.blocks.len() >3 && leader_stop_timer.elapsed() >= Duration::from_secs(60){
                print!("Leader {:?} is going to sleep for 60 seconds - new election should be held.\n", x.raft.id);
                thread::sleep(Duration::from_secs(30));
                leader_stop_timer = Instant::now();
            }
            raft_node.on_ready(&proposals, &conf_change_proposals);

        }

    }
}

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


fn handle_update(raft_node: &mut RaftNode, update: Update) -> bool {
    match update {
        Update::BlockNew(block) => raft_node.on_block_new(block),
        Update::RaftMessage(message) => raft_node.on_raft_message(&message.content),
        //Update::Shutdown => return false,

        update => warn!("Unhandled update: {:?}", update),
    }
    true
}





