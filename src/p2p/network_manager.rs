use std::collections::HashMap;

use crate::p2p::peer::Peer;
use zmq::{Context, Sendable};
use std::thread;
use crate::Update;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::RwLock;

pub struct NetworkManager {
    pub peers: HashMap<u64, Peer>,
    pub zero_mq_context: Context,
    pub network_manager_sender: Sender<NetworkManagerMessage>,
    network_manager_receiver: Receiver<NetworkManagerMessage>,
    raft_engine_sender: Option<Sender<Update>>

}

impl NetworkManager {
    pub fn new() -> Self{
        let (network_manager_sender, network_manager_receiver) = mpsc::channel();

        NetworkManager {
            peers: HashMap::new(),
            zero_mq_context: Default::default(),
            network_manager_sender,
            network_manager_receiver,
            raft_engine_sender: None
        }
    }

    pub fn add_new_peer(
        &mut self,
        port: u64
    ){
        self.peers.insert(port,Peer::new(port,&self.zero_mq_context));
    }

    pub fn start(&self) {
        loop {
            // Step raft messages.
            match self.network_manager_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(message) => {
                    debug!("Update: {:?}", message);
                    self.process_request(message);
                }
            }
        }
    }

    pub fn listen(&self, port: u64) -> Receiver<Update>{

        let (zeromq_sender, zeromq_reciever) = mpsc::channel();

        let router_socket = self.zero_mq_context.socket(zmq::ROUTER).unwrap();
        router_socket
            .bind(format!("tcp://*:{}", port.to_string()).as_ref())
            .expect("Node failed to bind router socket");

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

        zeromq_reciever
    }
}