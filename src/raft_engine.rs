use std::sync::mpsc::{Receiver, TryRecvError, Sender};
use crate::{RaftNode, Proposal, Block, RaftMessage, LeaderState, NodeConfig};
use std::thread;
use std::time::{Duration, Instant};
use raft::{StateRole};
use std::sync::{Arc, RwLock, mpsc};
use std::collections::VecDeque;
use crate::p2p::network_manager::{NetworkManagerMessage, BroadCastRequest, NetworkMessageType, NewBlockInfo};
use crate::blockchain::block::{BlockType, ConfiglBlockBody};
use crate::Blockchain;


pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(50);


pub struct RaftEngine {
    pub proposals_global: VecDeque<Proposal>,
    pub conf_change_proposals: VecDeque<Proposal>,
    network_manager_sender: Sender<NetworkManagerMessage>,
    pub raft_engine_client: Sender<RaftNodeMessage>,
    raft_engine_receiver: Receiver<RaftNodeMessage>,
    config: Arc<NodeConfig>,
}

impl RaftEngine {

    pub fn new(
        network_manager: Sender<NetworkManagerMessage>,
        node_config: Arc<NodeConfig>,
    ) -> Self{
        let (tx, rx) = mpsc::channel();
        RaftEngine {
            proposals_global : VecDeque::<Proposal>::new(),
            conf_change_proposals: VecDeque::<Proposal>::new(),
            network_manager_sender: network_manager,
            raft_engine_client: tx,
            raft_engine_receiver: rx,
            config: node_config
        }
    }

    pub fn start(
        &mut self,
        block_chain: Arc<RwLock<Blockchain>>,
        genesis_config: ConfiglBlockBody
    ){
        let mut raft_tick_timer = Instant::now();
        let mut new_block_timer = Instant::now();

        let mut raft_node = RaftNode::new(
            self.config.node_id,
            self.network_manager_sender.clone(),
            genesis_config.clone(),
            block_chain.clone(),
        );

        loop {
            // Step raft messages.
            match self.raft_engine_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(update) => {
                    //debug!("Update: {:?}", update);
                    self.handle_update( &mut raft_node, update);
                }

            }
            thread::sleep(Duration::from_millis(10));
            let raft = match raft_node.raw_node {
                Some(ref mut r) => r,
                // When Node::raft is `None` it means the the node was not initialized
                _ => continue,
            };

            if raft_tick_timer.elapsed() >= RAFT_TICK_TIMEOUT {
                // Tick the raft.
                raft.tick();
                // Reset timer
                raft_tick_timer = Instant::now();
            }
            if raft.raft.state == StateRole::Leader {
                // Handle new proposals.
                for p in self.proposals_global.iter_mut().skip_while(|p| p.proposed > 0) {
                    raft_node.propose(p);
                }
                for p in self.conf_change_proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                    raft_node.propose(p);
                }
                //Create new new blocK if:
                //  - node is a leader
                //  - time to create new block elapsed
                //  - previous block was committed (leader is not proposing previous block)
                if new_block_timer.elapsed() >= Duration::from_millis(self.config.pace_of_block_creation)
                    && match raft_node.leader_state { Some(LeaderState::Proposing) => false, _ => true }
                {
                    let new_block_id;
                    let new_block;
                    if let Some(last_block) = block_chain.read().expect("BlockChain Lock is poisoned").get_chain_head() {
                        new_block_id = last_block.header.block_id + 1;
                        new_block = Block::new(new_block_id, 1,BlockType::Normal,
                                               0,last_block.hash(), self.config.node_id, &self.config.key_pair);

                    }else {
                        //First block - genesis
                        new_block_id = 1;
                        new_block = Block::genesis(genesis_config.clone(), self.config.node_id, &self.config.key_pair)
                    }
                    info!("| ---------------------- |");
                    info!("| Created new block - {} {}|",new_block_id, new_block.hash());
                    info!("| ---------------------- |");
                    //Add block to uncommitted block que
                    block_chain.write().expect("BlockChain Lock is poisoned").add_to_uncommitted_block_que(new_block.clone());

                    //Update leader state - leader is proposing block
                    raft_node.leader_state = Some(LeaderState::Proposing);
                    let (proposal, _rx) = Proposal::new_block(new_block.clone());
                    self.proposals_global.push_back(proposal);

                    let message_to_send = NetworkMessageType::BlockNew(NewBlockInfo::new(self.config.node_id,new_block.header.block_id, new_block.hash()));
                    self.network_manager_sender.send(NetworkManagerMessage::BroadCastRequest(BroadCastRequest::new(message_to_send))).unwrap();

                    new_block_timer = Instant::now();
                }
            }

            // let x = match raft_node.raw_node {
            //     Some(ref mut r) => r,
            //     // When Node::raft is `None` it means the the node was not initialized
            //     _ => continue,
            // };
            //
            //
            // //If node is follower - reset leader_stop_timer every time
            // if x.raft.state == StateRole::Follower{
            //     leader_stop_timer = Instant::now();
            // }
            // //if node is Leader for longer then 60 seconds - sleep node threed, new leader should
            // //be elected and after wake up this node should catch current log and blockchain state
            // if x.raft.state == StateRole::Leader && block_chain.read().expect("BlockChain Lock is poisoned").blocks.len() >3 && leader_stop_timer.elapsed() >= Duration::from_secs(60){
            //     info!("[SLEEP] Leader {:?} is going to sleep for 60 seconds - new election should be held.\n", x.raft.id);
            //     thread::sleep(Duration::from_secs(30));
            //     leader_stop_timer = Instant::now();
            // }
            raft_node.on_ready();

        }

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
}

#[derive(Debug)]
pub enum RaftNodeMessage{
    RaftMessage(RaftMessage),
    BlockNew(Block),
}









