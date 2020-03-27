use crate::p2p::network_manager::NetworkManager;
use crate::raft_engine::{RaftEngine, RaftNodeMessage};
use std::thread;
use std::sync::{Mutex, mpsc};
use crate::{Proposal, Update, Blockchain};
use std::collections::VecDeque;
use raft::{prelude::*, StateRole};
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};


pub struct Node{
    this_peer_port: u64,
    node_client: Sender<Update>,
    node_receiver: Receiver<Update>,
    raft_engine_client: Option<Sender<RaftNodeMessage>>,
    pub blockchain: Blockchain,
}

impl Node{

    pub fn new(this_peer_port: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        Node{
            this_peer_port,
            node_client: tx,
            node_receiver: rx,
            raft_engine_client: None,
            blockchain: Blockchain::new(),
        }
    }

    pub fn start(&mut self, this_peer_port: u64, is_raft_node: bool, is_leader: bool, peers: Vec<u64>) {

        let mut network_manager = NetworkManager::new(self.node_client.clone());

        let raft_engine = match is_raft_node {
            true => Some(RaftEngine::new(network_manager.network_manager_sender.clone())),
            _ => None
        };

        match raft_engine{
            Some(ref engine ) =>  self.raft_engine_client = Some(engine.raft_engine_client.clone()),
            _ => {}
        }

        let peers_net_manager = peers.clone();
        let peers_raft = peers.clone();

        let handle = thread::spawn( move ||
            network_manager.start(this_peer_port,peers_net_manager)
        );

        if(is_raft_node){

            let test = raft_engine.is_some();
            let mut raft_engine = raft_engine.expect("Raft engine is not initialized");
            let raft_conf_proposals = raft_engine.conf_change_proposals_global.clone();


            let handle = thread::spawn( move || {
                raft_engine.start(this_peer_port,is_leader,peers.clone());
            }

            );

            if is_leader {
                thread::spawn( move || {
                        for peer in peers_raft.iter() {
                            add_new_raft_node(raft_conf_proposals.as_ref(), *peer);
                        }
                    }
                );
            }
        }

        loop{
            match self.node_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(update) => {
                    match update {
                        Update::BlockNew(block) => {
                            println!("Received information about new block:{:?}",block);
                            if self.raft_engine_client.is_some(){
                                self.raft_engine_client.as_ref().unwrap().send(RaftNodeMessage::BlockNew(block));
                            }
                        },
                        Update::RaftMessage(raft_message) => {
                            if self.raft_engine_client.is_some(){
                                self.raft_engine_client.as_ref().unwrap().send(RaftNodeMessage::RaftMessage(raft_message));
                            }
                        }
                        update => warn!("Unhandled update: {:?}", update),
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