
use zmq::{Socket, Context};


pub struct Peer {
    pub peer_ip: String,
    pub peer_port: u64,
    pub peer_id: u64,
    pub socket: Socket,
}

impl Peer {
    pub fn new(
        peer_ip: String,
        peer_id: u64,
        peer_port: u64,
        context: &Context
    ) -> Self {
        let dealer_socket = context.socket(zmq::DEALER).unwrap();
        dealer_socket
            .connect(format!("tcp://{}:{}", peer_ip, peer_port.to_string()).as_ref())
            .expect("Failed to connect to peer");

        Peer {
            peer_ip: peer_ip,
            peer_port: peer_port,
            peer_id: peer_id,
            socket: dealer_socket,
        }
    }
}