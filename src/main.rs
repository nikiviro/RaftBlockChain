use std::env;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{str, thread};
use prost::Message as ProstMsg;
use raft::eraftpb::ConfState;
use raft::storage::MemStorage;
use raft::{prelude::*, StateRole};
use std::collections::hash_map::RandomState;
use std::time::{ SystemTime, UNIX_EPOCH };
use std::process::exit;

mod blockchain;

pub use crate::blockchain::block::Block;
pub use crate::blockchain::*;

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

    let tick_interval = 10;


    //create "num_of_nodes" channels, so the nodes can communicate with each other
    let (mut tx_vec, mut rx_vec) = (Vec::new(), Vec::new());
    for _ in 0..num_of_nodes {
        let (tx, rx) = mpsc::channel();
        tx_vec.push(tx);
        rx_vec.push(rx);
    }

    let proposals = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));


    //store handle for each thread
    let mut handles = Vec::new();

    let mut current_node_id: u64 = 1;
    for (i, rx) in rx_vec.into_iter().enumerate() {

        //"mailboxes" for each node - [{node_id,tx},{node_id,tx}...]
        let mailboxes: HashMap<u64, mpsc::Sender<Message<>>> = (1..num_of_nodes+1u64).zip(tx_vec.iter().cloned()).collect();
        println!("Mailboxes: {:?}",mailboxes);

        let mut node = match i {
            // Create node 1 as leader
            0 => Node::create_raft_leader(1, rx, mailboxes),
            // Other nodes are followers.
            _ => Node::create_raft_follower(current_node_id,rx, mailboxes),
        };

        let proposals = Arc::clone(&proposals);

        let mut t = Instant::now();

        let handle = thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(10));

            loop {
                // Step raft messages.
                match node.my_mailbox.try_recv() {
                    Ok(msg) => node.step(msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            let raft = match node.raft {
                Some(ref mut r) => r,
                // When Node::raft is `None` it means the the node was not initialized
                _ => continue,
            };

            if t.elapsed() >= Duration::from_millis(tick_interval) {
                // Tick the raft.
                raft.tick();
                t = Instant::now();
            }

            // Let the leader pick pending proposals from the global queue.
            if raft.raft.state == StateRole::Leader {
                // Handle new proposals.
                let mut proposals = proposals.lock().unwrap();
                for p in proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                    propose(raft, p);
                }
            }

            //stop leader 1 for 30 seconds - new election should be held
            if raft.raft.state == StateRole::Leader && node.blockchain.blocks.len()>3 && raft.raft.id == 1 {
                thread::sleep(Duration::from_secs(30));
            }

            on_ready(raft, &mut node.blockchain, &node.mailboxes, &proposals);

        }); handles.push(handle);
        current_node_id+=1;
    }

    for i in 2..num_of_nodes+1u64 {
        add_new_node(proposals.as_ref(),i);
    }

    //add new block every 20 seconds
    let mut block_index = 1;
    loop{
        println!("----------------------");
        println!("Adding new block - {}",block_index);
        println!("----------------------");
        let new_block = Block::new(block_index, 1,1,0,"1".to_string(),now(), vec![0; 32], vec![0; 32], vec![0; 64]);
        let (proposal, rx) = Proposal::normal(new_block);
        proposals.lock().unwrap().push_back(proposal);
        // After we got a response from `rx`, we can assume that block was inserted successfully to the blockchain
        rx.recv().unwrap();
        block_index+=1;
        thread::sleep(Duration::from_secs(20));

    }

    for th in handles {
        th.join().unwrap();
    }

}

struct Node {
    // None if the raft is not initialized.
    id: u64,
    raft: Option<RawNode<MemStorage>>,
    my_mailbox: Receiver<Message>,
    mailboxes: HashMap<u64, Sender<Message>>,
    // Key-value pairs after applied. `MemStorage` only contains raft logs,
    // so we need an additional storage engine.
    blockchain: Blockchain,
}

impl Node {
    // Create a raft leader only with itself in its configuration.
    fn create_raft_leader(
        id: u64,
        my_mailbox: Receiver<Message>,
        mailboxes: HashMap<u64, Sender<Message>>,
    ) -> Self {
        let mut cfg = Config {
            election_tick: 10,
            heartbeat_tick: 3,
            id: id,
            tag: format!("raft_node{}", id),
            ..Default::default()
        };

        let storage = MemStorage::new_with_conf_state(ConfState::from((vec![id], vec![])));
        let raft = Some(RawNode::new(&cfg, storage).unwrap());
        Node {
            id,
            raft,
            my_mailbox,
            mailboxes,
            blockchain: Blockchain::new(),
        }
    }

    // Create a raft follower.
    fn create_raft_follower(
        id: u64,
        my_mailbox: Receiver<Message>,
        mailboxes: HashMap<u64, Sender<Message>>,
    ) -> Self {
        Node {
            id,
            raft: None,
            my_mailbox,
            mailboxes,
            blockchain: Blockchain::new(),
        }
    }
    // Step a raft message, initialize the raft if need.
    fn step(&mut self, msg: Message) {
        if self.raft.is_none() {
            if is_initial_msg(&msg) {
                self.initialize_raft_from_message(&msg);
            } else {
                return;
            }
        }
        let raft = self.raft.as_mut().unwrap();
        let _ = raft.step(msg);
    }

    fn initialize_raft_from_message(&mut self, msg: &Message) {
        if !is_initial_msg(msg) {
            return;
        }
        let mut cfg =  Config {
            election_tick: 10,
            heartbeat_tick: 3,
            id: msg.get_to(),
            tag: format!("raft_node{}", msg.get_to()),
            ..Default::default()
        };
        let storage = MemStorage::new();
        self.raft = Some(RawNode::new(&cfg, storage).unwrap());
    }
}

struct Proposal {
    normal: Option<Block>, // key is an u16 integer, and value is a string.
    conf_change: Option<ConfChange>, // conf change.
    proposed: u64,
    propose_success: SyncSender<bool>,
}

impl Proposal {
    fn conf_change(cc: &ConfChange) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            normal: None,
            conf_change: Some(cc.clone()),
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }

    fn normal(block: Block) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            normal: Some(block),
            conf_change: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }
}


fn propose(raft_group: &mut RawNode<MemStorage>, proposal: &mut Proposal) {
    let last_index1 = raft_group.raft.raft_log.last_index() + 1;
    if let Some(ref block) = proposal.normal {
        //convert Block struct to bytes
        let data = bincode::serialize(&block).unwrap();
        let _ = raft_group.propose(vec![], data);
    } else if let Some(ref cc) = proposal.conf_change {
        let _ = raft_group.propose_conf_change(vec![], cc.clone());
    }

    let last_index2 = raft_group.raft.raft_log.last_index() + 1;
    if last_index2 == last_index1 {
        // Propose failed, don't forget to respond to the client.
        proposal.propose_success.send(false).unwrap();
    } else {
        proposal.proposed = last_index1;
    }
}

fn on_ready(
    raft: &mut RawNode<MemStorage>,
    blockchain: &mut Blockchain,
    mailboxes: &HashMap<u64, Sender<Message>>,
    proposals: &Mutex<VecDeque<Proposal>>,
) {
    if !raft.has_ready() {
        return;
    }
    let store = raft.raft.raft_log.store.clone();

    // Get the `Ready` with `RawNode::ready` interface.
    let mut ready = raft.ready();

    // Persistent raft logs. It's necessary because in `RawNode::advance` we stabilize
    // raft logs to the latest position.
    if let Err(e) = store.wl().append(ready.entries()) {
        error!("persist raft log fail: {:?}, need to retry or panic", e);
        return;
    }

    // Apply the snapshot. It's necessary because in `RawNode::advance` we stabilize the snapshot.
    if *ready.snapshot() != Snapshot::new_() {
        let s = ready.snapshot().clone();
        if let Err(e) = store.wl().apply_snapshot(s) {
            error!("apply snapshot fail: {:?}, need to retry or panic", e);
            return;
        }
    }

    // Send out the messages come from the node.
    for msg in ready.messages.drain(..) {
        let to = msg.get_to();
        if mailboxes[&to].send(msg).is_err() {
            warn!("send raft message to {} fail, let Raft retry it", to);
        }
    }

    // Apply all committed proposals.
    if let Some(committed_entries) = ready.committed_entries.take() {
        for entry in &committed_entries {
            if entry.get_data().is_empty() {
                // From new elected leaders.
                continue;
            }
            if let EntryType::EntryConfChange = entry.get_entry_type() {
                // Handle conf change messages
                let mut cc = ConfChange::new_();
                ProstMsg::merge(&mut cc, entry.get_data()).unwrap();
                let node_id = cc.get_node_id();
                match cc.get_change_type() {
                    ConfChangeType::AddNode => raft.raft.add_node(node_id).unwrap(),
                    ConfChangeType::RemoveNode => raft.raft.remove_node(node_id).unwrap(),
                    ConfChangeType::AddLearnerNode => raft.raft.add_learner(node_id).unwrap(),
                    ConfChangeType::BeginMembershipChange
                    | ConfChangeType::FinalizeMembershipChange => unimplemented!(),
                }
                let cs = ConfState::from(raft.raft.prs().configuration().clone());
                store.wl().set_conf_state(cs, None);

            }  else {
                // For normal proposals, extract block from message
                // insert block into the blockchain
                let block: Block = bincode::deserialize(&entry.get_data()).unwrap();
                let block_index = block.block_id;
                blockchain.add_block(block);
                let node_role;
                if raft.raft.state == StateRole::Leader{
                    node_role = String::from("Leader");
                }else{
                    node_role = String::from("Follower");
                }
                println!("Node {} - {} added new block at index {}",raft.raft.id, node_role,block_index);
            }
            if raft.raft.state == StateRole::Leader {
                // The leader should response to the clients, tell them if their proposals
                // succeeded or not.
                let proposal = proposals.lock().unwrap().pop_front().unwrap();
                proposal.propose_success.send(true).unwrap();
            }
        }
        if let Some(last_committed) = committed_entries.last() {
            let mut s = store.wl();
            s.mut_hard_state().set_commit(last_committed.get_index());
            s.mut_hard_state().set_term(last_committed.get_term());
        }
    }
    // Call `RawNode::advance` interface to update position flags in the raft.
    raft.advance(ready);
}



// The message can be used to initialize a raft node or not.
fn is_initial_msg(msg: &Message) -> bool {
    let msg_type = msg.get_msg_type();
    msg_type == MessageType::MsgRequestVote
        || msg_type == MessageType::MsgRequestPreVote
        || (msg_type == MessageType::MsgHeartbeat && msg.get_commit() == 0)
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