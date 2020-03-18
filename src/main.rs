extern crate bincode;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::thread;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::process::exit;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use config::{Config as ConfigLoader, File};
use protobuf::{self, Message as ProtobufMessage};
use raft::{prelude::*, StateRole};
use zmq::Socket;

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;
pub use crate::node::*;
pub use crate::node::Node;
pub use crate::proposal::Proposal;
use crate::p2p::network_manager::NetworkManager;
use crate::raft_engine::RaftEngine;

mod blockchain;
mod node;
mod proposal;
mod p2p;
mod raft_engine;

pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(100);


#[derive(Serialize, Deserialize,Debug)]
pub struct ConfigStruct {
    pub nodes_without_raft: Vec<String>,
}

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len()<2 {
        eprintln!("Problem parsing arguments: You need to specify how many nodes should be in cluster.  ",);
        eprintln!("Example : Cargo run 'num_of_nodes'");

        exit(1);
    }
    let is_leader_arg: u8= args[1].parse().unwrap();
    let is_leader = is_leader_arg != 0;
    let this_peer_port: u64 = args[2].parse().unwrap();
    let mut peer_list = Vec::new();
    for x in 3..(args.len()){
        println!("Peer: {}", args[x]);
        let peer_port: u64 = args[x].parse().unwrap();
        peer_list.push(peer_port);
    }


    let mut is_node_without_raft = false;
    let config = load_config_from_file();

    if config.nodes_without_raft.contains(&this_peer_port.to_string()){
        is_node_without_raft = true;
        println!("No-RAFT node")
    }

    //create NetworkManager - store Peers connection and ZeroMq context
    let mut network_manager = NetworkManager::new();

    for peer_port in peer_list.iter(){
        network_manager.add_new_peer(peer_port.clone());
    }


    let mut raft_engine = RaftEngine::new(network_manager.network_manager_sender.clone());
    network_manager.set_raft_engine_sender(raft_engine.raft_engine_client.clone());

    //Network manager will crate ZeroMq ROUTER socket and listen on specified port
    //call to network_manager.listen() returns channel Receiver - messages received on ROUTER socket
    //will be forwarded through this channel.
    let zeromq_reciever = network_manager.listen(this_peer_port);

    let test = raft_engine.conf_change_proposals_global.clone();
    let test2 = peer_list.clone();

    thread::spawn( move ||
        network_manager.start()
    );

    if(!is_node_without_raft){

        let handle = thread::spawn( move || {
            raft_engine.start(this_peer_port,is_leader,peer_list);
        }

    );

        if is_leader {
            for peer in test2.iter() {
                add_new_node(test.as_ref(), *peer);
            }
        }

        handle.join().unwrap();

    }
    else{
        loop{
            match zeromq_reciever.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(update) => {
                    match update {
                        Update::BlockNew(block) => {
                            println!("Received information about new block:{:?}",block)
                        }
                        update => warn!("Unhandled update: {:?}", update),
                    }
                    //debug!("Update: {:?}", update);
                }
            }
        }
    }
}

fn handle_update(node: &mut Node, update: Update) -> bool {
    match update {
        Update::BlockNew(block) => node.on_block_new(block),
        Update::RaftMessage(message) => node.on_raft_message(&message.content),
        //Update::Shutdown => return false,

        update => warn!("Unhandled update: {:?}", update),
    }
    true
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

pub fn load_config_from_file() -> ConfigStruct {

    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name("config/config.json")).unwrap();
    // Print out our settings (as a HashMap)
    let configstruct =  config.try_into::<ConfigStruct>().unwrap();
    println!("\n{:?} \n\n-----------",
             &configstruct);

    configstruct
}

pub fn now () -> u128 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        ;

    duration.as_secs() as u128 * 1000 + duration.subsec_millis() as u128
}