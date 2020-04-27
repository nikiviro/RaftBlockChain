use std::collections::HashMap;

use crate::p2p::peer::Peer;
use zmq::{Context, Sendable};
use std::thread;
use crate::{Update, Block, RaftMessage, ConfigStructJson, NodeConfig};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{RwLock, Arc};
use crate::node::NodeMessage;
use ed25519_dalek::{Keypair, Signature, PublicKey};

pub struct NetworkManager {
    pub peers: HashMap<u64, Peer>,
    pub zero_mq_context: Context,
    pub network_manager_sender: Sender<NetworkManagerMessage>,
    network_manager_receiver: Receiver<NetworkManagerMessage>,
    node_client: Sender<NodeMessage>,
    config: Arc<NodeConfig>
}

impl NetworkManager {
    pub fn new(node_sender: Sender<NodeMessage>, config: Arc<NodeConfig>) -> Self{
        let (network_manager_sender, network_manager_receiver) = mpsc::channel();

        NetworkManager {
            peers: HashMap::new(),
            zero_mq_context: Default::default(),
            network_manager_sender,
            network_manager_receiver,
            node_client: node_sender,
            config: config
        }
    }

    pub fn add_new_peer(
        &mut self,
        port: u64
    ){
        self.peers.insert(port,Peer::new(port,&self.zero_mq_context));
    }

    pub fn start(&mut self) {

        for (peer_port, address) in self.config.nodes_to_connect.clone(){
            self.add_new_peer(peer_port.clone());
        }
        self.listen(self.config.node_id);

        loop {
            match self.network_manager_receiver.try_recv() {
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => return,
                Ok(message) => {
                    //debug!("NetworkManagerMessage received: {:?}", message);
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
        let elector_list = self.config.electors.clone();
        let receiver_thread_handle = thread::spawn( move ||
            loop {
                let msq = router_socket.recv_multipart(0).unwrap();
                //println!("Received {:?}", msg);
                //thread::sleep(Duration::from_millis(1000));
                //responder.send("World", 0).unwrap();
                let data = &msq[1];
                let received_message: NetworkMessage = bincode::deserialize(&data).expect("Cannot deserialize update message");
                //zeromq_sender.send(received_message);
                handle_receieved_message(received_message, node_sender.clone(),&elector_list);

            }
        );
    }

    pub fn send_to(&self, request: SendToRequest){

        let network_message = NetworkMessage::new(self.config.node_id,request.to,request.data, &self.config.key_pair);
        let data = network_message.serialize();
        //let data =  bincode::serialize(&request.data).expect("Error while serializing message to send");
        self.peers[&request.to].socket.send(data, 0).unwrap();
    }

    pub fn send_broadcast(&self, request: BroadCastRequest){
        //let data =  bincode::serialize(&request.data).expect("Error while serializing message to send");

        for (id, peer) in self.peers.iter() {
            let network_message = NetworkMessage::new(self.config.node_id,id.clone(),request.data.clone(), &self.config.key_pair);
            let data = network_message.serialize();
            peer.socket.send(data.clone(),0);
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


fn handle_receieved_message (received_message: NetworkMessage, node_client: Sender<NodeMessage>, elector_list: &HashMap<u64, PublicKey>){

    match received_message.message_type {
        NetworkMessageType::BlockNew(block) => {
            node_client.send(NodeMessage::BlockNew(block));
        },
        NetworkMessageType::RaftMessage(ref raft_message) => {
            //Verify raft node signature
            let sender_id = received_message.from;
            match elector_list.get(&sender_id){
                Some(public_key) => {
                   match received_message.check_signature(&public_key){
                       true => {
                           debug!("[RAFT MESSAGE SIGNATURE CORRECT]")
                       },
                       false =>  {
                           debug!("[RAFT MESSAGE SIGNATURE INCORRECT]");
                           return;
                       }
                   }
                }
                None => {
                    debug!("[RAFT MESSAGE UKNOWN NODE] - Received raft message from node with unknown public key");
                    return;
                }
            }
            node_client.send(NodeMessage::RaftMessage(raft_message.clone()));
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
    data: NetworkMessageType,
}

impl SendToRequest{
    pub fn new ( to: u64, data: NetworkMessageType) -> Self{
        SendToRequest{
            to,
            data
        }
    }
}

#[derive(Debug)]
pub struct BroadCastRequest {
    data: NetworkMessageType
}

impl BroadCastRequest{
    pub fn new (data: NetworkMessageType) -> Self{
        BroadCastRequest{
            data
        }
    }
}

#[derive(Debug,Serialize, Deserialize, Clone)]
pub enum NetworkMessageType {
    BlockNew(Block),
    RaftMessage(RaftMessage)
}

#[derive(Debug,Serialize, Deserialize)]
pub struct NetworkMessage{
    from: u64,
    to: u64,
    message_type: NetworkMessageType,
    signature: Signature
}

impl NetworkMessage{
    pub fn new ( from: u64, to: u64,message_type: NetworkMessageType, key_pair: &Keypair) -> Self{
        let mut bytes = vec![];
        bytes.extend(bincode::serialize(&from).expect("Error while serializing 'from' message field"));
        bytes.extend(bincode::serialize(&to).expect("Error while serializing 'to' message field"));
        bytes.extend(bincode::serialize(&message_type).expect("Error while serializing 'message_type' message field"));

        NetworkMessage{
            from,
            to,
            message_type,
            signature: key_pair.sign(&bytes)
        }
    }
    pub fn get_bytes_for_signature(&self) -> Vec<u8>{
        let mut bytes = vec![];
        bytes.extend(bincode::serialize(&self.from).expect("Error while serializing 'from' message field"));
        bytes.extend(bincode::serialize(&self.to).expect("Error while serializing 'to' message field"));
        bytes.extend(bincode::serialize(&self.message_type).expect("Error while serializing 'message_type' message field"));
        bytes
    }
    pub fn serialize(&self) -> Vec<u8>{
        bincode::serialize(&self).expect("Error while serializing NetworkMessage struct")
    }
    pub fn check_signature(&self, public_key: &PublicKey) -> bool {
        public_key.verify(&self.get_bytes_for_signature(), &self.signature).is_ok()
    }
}