use std::env;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{ thread};
use raft::{prelude::*, StateRole};
use std::time::{ SystemTime, UNIX_EPOCH };
use std::process::exit;

mod blockchain;
mod node;
mod proposal;
pub use crate::blockchain::block::Block;
pub use crate::blockchain::*;

pub use crate::node::*;
pub use crate::node::Node;
pub use crate::proposal::Proposal;

pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(100);

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate bincode;

#[macro_use]
extern crate log;
extern crate env_logger;

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len()<2 {
        eprintln!("Problem parsing arguments: You need to specify how many nodes should be in cluster.  ",);
        eprintln!("Example : Cargo run 'num_of_nodes'");

        exit(1);
    }
    let num_of_nodes: u64 = args[1].parse().unwrap();
    //let initial_leader_node: u64 = args[2].parse().unwrap();;
    println!("Starting cluster with {} nodes...", num_of_nodes);

    //create "num_of_nodes" channels, so the nodes can communicate with each other
    let (mut tx_vec, mut rx_vec) = (Vec::new(), Vec::new());
    for _ in 0..num_of_nodes {
        let (tx, rx) = mpsc::channel();
        tx_vec.push(tx);
        rx_vec.push(rx);
    }

    //Que for storing blockchain updates requests (e.g adding new block)
    let blockchain_updates = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));


    //store handle for each thread
    let mut handles = Vec::new();

    let mut current_node_id: u64 = 1;
    for (i, rx) in rx_vec.into_iter().enumerate() {

        //"mailboxes" for each node - [{node_id,tx},{node_id,tx}...]
        let mailboxes: HashMap<u64, mpsc::Sender<Update<>>> = (1..num_of_nodes+1u64).zip(tx_vec.iter().cloned()).collect();
        println!("Mailboxes: {:?}",mailboxes);

        let mut node = match i {
            // Create node 1 as leader
            0 => Node::create_raft_leader(1, rx, mailboxes),
            // Other nodes are followers.
            _ => Node::create_raft_follower(current_node_id,rx, mailboxes),
        };

        let proposals = Arc::clone(&blockchain_updates);

        let mut t = Instant::now();
        let mut leader_stop_timer = Instant::now();


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
                //TODO: Make sure that new block block will be proposed after previus block was finally commited/rejecte by network
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
                // be elected and after wake up this node should catch current log and blockchain state
                if x.raft.state == StateRole::Leader && node.blockchain.blocks.len() >3 && leader_stop_timer.elapsed() >= Duration::from_secs(60){
                    print!("Leader {:?} is going to sleep for 60 seconds - new election should be held.\n", x.raft.id);
                    thread::sleep(Duration::from_secs(30));
                    leader_stop_timer = Instant::now();
                }


                node.on_ready( &proposals);

            });

        handles.push(handle);
        current_node_id+=1;
    }

    for i in 2..num_of_nodes+1u64 {
        add_new_node(blockchain_updates.as_ref(),i);
    }

    //add new block every 20 seconds
    let mut block_index = 1;
    loop{
        println!("----------------------");
        println!("Adding new block - {}",block_index);
        println!("----------------------");
        let new_block = Block::new(block_index, 1,1,0,"1".to_string(),now(), vec![0; 32], vec![0; 32], vec![0; 64]);
        let (proposal, rx) = Proposal::new_block(new_block);
        blockchain_updates.lock().unwrap().push_back(proposal);
        // After we got a response from `rx`, we can assume that block was inserted successfully to the blockchain
        rx.recv().unwrap();
        block_index+=1;
        thread::sleep(Duration::from_secs(20));
    }

    for th in handles {
        th.join().unwrap();
    }

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