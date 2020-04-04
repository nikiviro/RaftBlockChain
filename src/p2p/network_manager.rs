use std::collections::HashMap;

use crate::p2p::peer::Peer;
use zmq::{Context, Sendable};
use std::thread;
use crate::{Update, Block, RaftMessage};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::RwLock;
use crate::node::NodeMessage;

pub struct NetworkManager {
    pub peers: HashMap<u64, Peer>,
    pub zero_mq_context: Context,
    pub network_manager_sender: Sender<NetworkManagerMessage>,
    network_manager_receiver: Receiver<NetworkManagerMessage>,
    node_client: Sender<NodeMessage>,

}

impl NetworkManager {
    pub fn new(node_sender: Sender<NodeMessage>) -> Self{
        let (network_manager_sender, network_manager_receiver) = mpsc::channel();

        NetworkManager {
            peers: HashMap::new(),
            zero_mq_context: Default::default(),
            network_manager_sender,
            network_manager_receiver,
            node_client: node_sender
        }
    }

    pub fn add_new_peer(
        &mut self,
        port: u64
    ){
        self.peers.insert(port,Peer::new(port,&self.zero_mq_context));
    }

    pub fn start(&mut self, this_peer_port: u64,peer_list: Vec<u64>) {

        for peer_port in peer_list.iter(){
            self.add_new_peer(peer_port.clone());
        }
        self.listen(this_peer_port);

        loop {
            match self.network_manager_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(message) => {
                    debug!("NetworkManagerMessage received: {:?}", message);
                    self.process_request(message);
                }
            }
        }
    }

    pub fn listen(&self, port: u64){

        let router_socket = self.zero_mq_context.socket(zmq::ROUTER).unwrap();
        router_socket
            .bind(format!("tcp://*:{}", port.to_string()).as_ref())
            .expect("Node failed to bind router socket");


        //Create new thread in which we will listen for incoming zeromq messages from other peers
        //Received message will be forwarded through the channel to main thread
        let node_sender = self.node_client.clone();
        let receiver_thread_handle = thread::spawn( move ||
            loop {
                let msq = router_socket.recv_multipart(0).unwrap();
                //println!("Received {:?}", msg);
                //thread::sleep(Duration::from_millis(1000));
                //responder.send("World", 0).unwrap();
                let data = &msq[1];
                let received_message: NetworkMessage = bincode::deserialize(&data).expect("Cannot deserialize update message");
                //zeromq_sender.send(received_message);
                handle_receieved_message(received_message, node_sender.clone());

            }
        );
    }

    pub fn send_to(&self, request: SendToRequest){
        self.peers[&request.to].socket.send(request.data, 0).unwrap();
    }

    pub fn send_broadcast(&self, request: BroadCastRequest){
        for (id, peer) in self.peers.iter() {
            peer.socket.send(request.data.clone(),0);
        }
    }

    pub fn process_request(&self, message: NetworkManagerMessage){
        match message {
            NetworkManagerMessage::SendToRequest(request) => self.send_to(request),
            NetworkManagerMessage::BroadCastRequest(request) => self.send_broadcast(request),
            update => warn!("Unhandled update: {:?}", update),
        }
    }
}


fn handle_receieved_message (received_message: NetworkMessage, node_client: Sender<NodeMessage>){
    match received_message {
        NetworkMessage::BlockNew(block) => {
            node_client.send(NodeMessage::BlockNew(block));
        },
        NetworkMessage::RaftMessage(raft_message) => {
            node_client.send(NodeMessage::RaftMessage(raft_message));
        }
        _ => warn!("Unhandled network message received: {:?}", received_message),
    }
}

#[derive(Debug)]
pub enum NetworkManagerMessage {
    SendToRequest(SendToRequest),
    BroadCastRequest(BroadCastRequest)
}

#[derive(Debug)]
pub struct SendToRequest {
    to: u64,
    data: Vec<u8>,
}

impl SendToRequest{
    pub fn new ( to: u64, data: Vec<u8>) -> Self{
        SendToRequest{
            to,
            data
        }
    }
}

#[derive(Debug)]
pub struct BroadCastRequest {
    data: Vec<u8>,
}

impl BroadCastRequest{
    pub fn new (data: Vec<u8>) -> Self{
        BroadCastRequest{
            data
        }
    }
}

#[derive(Debug,Serialize, Deserialize)]
pub enum NetworkMessage{
    BlockNew(Block),
    RaftMessage(RaftMessage)
}