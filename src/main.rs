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
use std::sync::{Arc, Mutex};
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
use crate::p2p::networkManager::NetworkManager;

mod blockchain;
mod node;
mod proposal;
mod p2p;

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
    let is_leader: u8 = args[1].parse().unwrap();
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


    //Create a channel for communication between main thread and zeromq receiver thread
    let (zeromq_sender, zeromq_reciever) = mpsc::channel();

    //create NetworkManager - store Peers connection an ZeroMq context
    let mut network_manager = NetworkManager::new();


    let router_socket = network_manager.zero_mq_context.socket(zmq::ROUTER).unwrap();
    router_socket
        .bind(format!("tcp://*:{}", this_peer_port.to_string()).as_ref())
        .expect("Node failed to bind router socket");

    for peer_port in peer_list.iter(){
        network_manager.add_new_peer(peer_port.clone());
    }


    //Create new thread in which we will listen for incoming zeromq messages from other peers
    //Received message will be forwarded through the channel to main thread
    let receiver_thread_handle = thread::spawn( move ||
        loop {
            let msq = router_socket.recv_multipart(0).unwrap();
            //println!("Received {:?}", msg);
            //thread::sleep(Duration::from_millis(1000));
            //responder.send("World", 0).unwrap();
            let data = &msq[1];
            let received_message: Update = bincode::deserialize(&data).expect("Cannot deserialize update message");
            zeromq_sender.send(received_message);
        }
    );

    if(!is_node_without_raft){
        //Que for storing blockchain updates requests (e.g adding new block)
        let proposals_global = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));
        let conf_change_proposals_global = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));


        let proposals = Arc::clone(&proposals_global);
        let conf_change_proposals = Arc::clone(&conf_change_proposals_global);

        let mut t = Instant::now();
        let mut leader_stop_timer = Instant::now();
        let mut new_block_timer = Instant::now();


        let mut node = match is_leader {
            // Create node 1 as leader
            1 => Node::create_raft_leader(this_peer_port, zeromq_reciever, network_manager,peer_list.clone()),
            // Other nodes are followers.
            _ => Node::create_raft_follower(this_peer_port,zeromq_reciever, network_manager,true),
        };

        let mut proposal_responses = Vec::new();


        let handle = thread::spawn(move ||
            //Forever loop to drive the Raft
            loop {
                // Step raft messages.
                match node.my_mailbox.try_recv() {
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => return,
                    Ok(update) => {
                        debug!("Update: {:?}", update);
                        handle_update(&mut node, update);
                    }

                }
                thread::sleep(Duration::from_millis(10));
                let raft = match node.raw_node {
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
                        node.propose(p);
                    }
                    let mut conf_change_proposals = conf_change_proposals.lock().unwrap();
                    for p in conf_change_proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                        node.propose(p);
                    }

                    //Add new Block
                    if new_block_timer.elapsed() >= Duration::from_secs(20) {
                        let mut new_block_id;

                        if let Some(last_block) = node.blockchain.get_last_block() {
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

//                    for receiver in proposal_responses.iter() {
//                        match receiver.try_recv() {
//                            Err(TryRecvError::Empty) => (),
//                            Err(TryRecvError::Disconnected) => {println!("Proposal transmiter end is disconnected")},
//                            Ok(confirmation) => {
//                                if confirmation{
//                                    println!("Block was commited - client was informed");
//                                }
//                            }
//                        }
//                    }
                    }
                }

                let x = match node.raw_node {
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
                if x.raft.state == StateRole::Leader && node.blockchain.blocks.len() >3 && leader_stop_timer.elapsed() >= Duration::from_secs(60){
                    print!("Leader {:?} is going to sleep for 60 seconds - new election should be held.\n", x.raft.id);
                    thread::sleep(Duration::from_secs(30));
                    leader_stop_timer = Instant::now();
                }
                node.on_ready( &proposals, &conf_change_proposals);

            });

        if is_leader == 1 {
            for peer in peer_list.iter() {
                add_new_node(conf_change_proposals_global.as_ref(), *peer);
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