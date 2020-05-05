use crate::p2p::network_manager::{NetworkManager, RequestBlockMessage, NetworkMessageType, NetworkManagerMessage, SendToRequest, NewBlockInfo, BroadCastRequest};
use crate::raft_engine::{RaftEngine, RaftNodeMessage};
use std::thread;
use std::sync::{Mutex, mpsc, RwLock, Arc};
use crate::{Proposal, Update, Blockchain, Block, RaftMessage, ConfigStructJson, NodeConfig};
use std::collections::VecDeque;
use raft::{prelude::*, StateRole};
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
use crate::blockchain::block::ConfiglBlockBody;
use std::rc::Rc;


pub struct Node{
    node_client: Sender<NodeMessage>,
    node_receiver: Receiver<NodeMessage>,
    raft_engine_client: Option<Sender<RaftNodeMessage>>,
    config: Arc<NodeConfig>
}

impl Node{

    pub fn new(config: Arc<NodeConfig>) -> Self {
        let (tx, rx) = mpsc::channel();
        Node{
            node_client: tx,
            node_receiver: rx,
            raft_engine_client: None,
            config: config
        }
    }

    pub fn start(&mut self, genesis_config: ConfiglBlockBody) {

        let is_elecor_node = match genesis_config.list_of_elector_nodes.get(&self.config.node_id){
            Some(public_key) => {
                if &self.config.key_pair.public == public_key{
                    true
                }
                else{
                    false
                }
            }
            _ => false
        };

        let mut block_chain = Arc::new(RwLock::new(Blockchain::new()));

        let mut network_manager = NetworkManager::new(self.node_client.clone(), self.config.clone());

        let network_manager_sender = network_manager.network_manager_sender.clone();

        let raft_engine = match is_elecor_node {
            true => Some(RaftEngine::new(network_manager.network_manager_sender.clone(), self.config.clone())),
            _ => None
        };

        match raft_engine{
            Some(ref engine ) =>  self.raft_engine_client = Some(engine.raft_engine_client.clone()),
            _ => {}
        }

        let handle = thread::spawn( move ||
            network_manager.start()
        );

        if(is_elecor_node){

            let mut raft_engine = raft_engine.expect("Raft engine is not initialized");

            let block_chain_raft_engine = block_chain.clone();
            let handle = thread::spawn( move || {
                raft_engine.start( block_chain_raft_engine, genesis_config);
            }

            );
        }

        loop{
            match self.node_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(message) => {
                    match message {
                        NodeMessage::BlockNew(block_info) => {

                            let mut blockchain = block_chain.read().expect("BlockChain Lock is poisoned");
                            if !blockchain.is_known_block(&block_info.block_hash){
                                info!("[RECEIVED INFO ABOUT NEW BLOCK - {} - REQUESTING BLOCK]", block_info.block_hash);
                                let message_to_send = NetworkMessageType::RequestBlock(RequestBlockMessage::new(self.config.node_id,block_info.block_id, block_info.block_hash));
                                //Request block from node that sended BlockNew message
                                network_manager_sender.send(NetworkManagerMessage::SendToRequest(SendToRequest::new(block_info.from,message_to_send)));
                            }
                        }
                        NodeMessage::RaftMessage(raft_message) => {
                            if self.raft_engine_client.is_some(){
                                self.raft_engine_client.as_ref().unwrap().send(RaftNodeMessage::RaftMessage(raft_message));
                            }
                        },
                        NodeMessage::RequestBlock(block_request) => {

                            let blockchain = block_chain.read().expect("BlockChain Lock is poisoned");
                            if let Some(block) = blockchain.find_block(block_request.block_id, block_request.block_hash) {
                                debug!("[RECEIVED BLOCKREQUEST FROM {} - HAVE REQUESTED BLOCK] Sending RequestBlockMessageResponse {:?}",block_request.requester_id, block);
                                let message_to_send = NetworkMessageType::RequestBlockResponse(block);
                                network_manager_sender.send(NetworkManagerMessage::SendToRequest(SendToRequest::new(block_request.requester_id, message_to_send)));

                            }else{
                                debug!("[DONT HAVE REQUESTED BLOCK]");
                            }
                        },
                        NodeMessage::RequestBlockResponse(block) => {
                            let block_hash = block.hash();
                            let mut blockchain = block_chain.write().expect("BlockChain Lock is poisoned");

                            if !blockchain.is_known_block(&block_hash){
                                info!("[RECEIVED BLOCK - {}] Received new block {:?}", block.hash(), block);
                                if block.is_valid(&self.config.electors){
                                    blockchain.add_to_uncommitted_block_que( block.clone());
                                    //self.blockchain.write().expect("Blockchain is poisoned").add_block(block);
                                    if self.raft_engine_client.is_some(){
                                        self.raft_engine_client.as_ref().unwrap().send(RaftNodeMessage::BlockNew(block.clone()));
                                    }

                                    let message_to_send = NetworkMessageType::BlockNew(NewBlockInfo::new(self.config.node_id,block.header.block_id, block.hash()));
                                    network_manager_sender.send(NetworkManagerMessage::BroadCastRequest(BroadCastRequest::new(message_to_send)));
                                }
                            }
                        },
                        _ => warn!("Unhandled update: {:?}", message),
                    }
                    //debug!("Update: {:?}", update);
                }
            }
        }


    }

//    pub fn start(&mut self){
//
//        let handle = thread::spawn( move ||
//            self.network_manager.start(vec![1, 2, 3])
//        );
//
//        handle.join();
//
//    }
}

#[derive(Debug,Serialize, Deserialize)]
pub enum NodeMessage {
    BlockNew(NewBlockInfo),
    RaftMessage(RaftMessage),
    RequestBlock(RequestBlockMessage),
    RequestBlockResponse(Block),
}

fn add_new_raft_node(proposals: &Mutex<VecDeque<Proposal>>, node_id: u64) {
    let mut conf_change = ConfChange::default();
    conf_change.set_node_id(node_id);
    conf_change.set_change_type(ConfChangeType::AddNode);
    let (proposal, rx) = Proposal::conf_change(&conf_change);
    proposals.lock().unwrap().push_back(proposal);
    if rx.recv().unwrap() {
        println!("Node {:?} succesfully added to cluster", node_id);
    }
}