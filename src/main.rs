use std::env;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{ thread};
use raft::{prelude::*, StateRole};
use std::time::{ SystemTime, UNIX_EPOCH };
use std::process::exit;
use protobuf::{self, Message as ProtobufMessage};


mod blockchain;
mod node;
mod proposal;
pub use crate::blockchain::block::Block;
pub use crate::blockchain::*;

pub use crate::node::*;
pub use crate::node::Node;
pub use crate::proposal::Proposal;
use zmq::Socket;

pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(100);

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate bincode;

#[macro_use]
extern crate log;
extern crate env_logger;


pub struct RaftPeer {
    pub peer_port: u64,
    pub socket: Socket,
}

impl RaftPeer {
    pub fn create_peer(
        peer_port: u64,
        socket: Socket,
    ) -> Self {

        RaftPeer {
            peer_port,
            socket,
        }
    }
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


    //Que for storing blockchain updates requests (e.g adding new block)
    let proposals_global = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));


    let proposals = Arc::clone(&proposals_global);

    let mut t = Instant::now();
    let mut leader_stop_timer = Instant::now();


    //Create a channel for communication between main thread and zeromq receiver thread
    let (zeromq_sender, zeromq_reciever) = mpsc::channel();

    //Create zeromQ context
    let context = zmq::Context::new();
    //Zeormq router socket - "RAFT PEER ENDPOINT"
    //Other peers will connect to this socket
    let router_socket = context.socket(zmq::ROUTER).unwrap();
    router_socket
        .bind(format!("tcp://*:{}", this_peer_port.to_string()).as_ref())
        .expect("Node failed to bind router socket");

    //Create sockets for all peers
    //Save peers into hashmap named peers
    let mut peers = HashMap::new();
    for peer_port in peer_list.iter(){
        let dealer_socket = context.socket(zmq::DEALER).unwrap();
        dealer_socket
            .connect(format!("tcp://localhost:{}", peer_port.to_string()).as_ref())
            .expect("Failed to connect to peer");
        peers.insert(peer_port.clone(), RaftPeer::create_peer(peer_port.clone(), dealer_socket));
    }

    let mut node = match is_leader {
        // Create node 1 as leader
        1 => Node::create_raft_leader(this_peer_port, zeromq_reciever, peers),
        // Other nodes are followers.
        _ => Node::create_raft_follower(this_peer_port,zeromq_reciever, peers),
    };

    //Create new thread in which we will listen for incoming zeromq messages from other peers
    //Received message will be forwarded through the channel to main thread
    let receiver_thread_handle = thread::spawn( move ||
        loop {
            let msq = router_socket.recv_multipart(0).unwrap();
            //println!("Received {:?}", msg);
            //thread::sleep(Duration::from_millis(1000));
            //responder.send("World", 0).unwrap();
            let data = &msq[1];
            let received_message: Message = protobuf::parse_from_bytes(&data).unwrap();
            zeromq_sender.send(Update::RaftMessage(received_message));
        }
    );


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


            node.on_ready( &proposals);

        });

    if is_leader == 1 {
        for peer in peer_list.iter() {
            add_new_node(proposals_global.as_ref(), *peer);
        }
    }

    //add new block every 20 seconds
    let mut block_index = 1;
    loop{
        if is_leader == 1 {
            println!("----------------------");
            println!("Adding new block - {}",block_index);
            println!("----------------------");
            let new_block = Block::new(block_index, 1,1,0,"1".to_string(),now(), vec![0; 32], vec![0; 32], vec![0; 64]);
            let (proposal, rx) = Proposal::new_block(new_block);
            proposals_global.lock().unwrap().push_back(proposal);
            // After we got a response from `rx`, we can assume that block was inserted successfully to the blockchain
            rx.recv().unwrap();
            println!("Client recieved confirmation about block insertion.");
            block_index+=1;
        }
        thread::sleep(Duration::from_secs(20));
    }

        handle.join().unwrap();

}

fn handle_update(node: &mut Node, update: Update) -> bool {
    match update {
        Update::BlockNew(block) => node.on_block_new(block),
        Update::RaftMessage(message) => node.step(message),
        //Update::Shutdown => return false,

        update => warn!("Unhandled update: {:?}", update),
    }
    true
}

fn add_new_node(proposals: &Mutex<VecDeque<Proposal>>, node_id: u64) {
    let mut conf_change = ConfChange::default();
    conf_change.set_node_id(node_id);
    conf_change.set_change_type(ConfChangeType::AddNode);
    loop {
        let (proposal, rx) = Proposal::conf_change(&conf_change);
        proposals.lock().unwrap().push_back(proposal);
        if rx.recv().unwrap() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn now () -> u128 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        ;

    duration.as_secs() as u128 * 1000 + duration.subsec_millis() as u128
}