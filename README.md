# RaftBlockChain
The following is implementation of a blockchain network prototype, which focuses on solving problems of communication between network nodes, reaching consensus and distribution of blocks between nodes. The prototype is implemented in the Rust programming language. In the created prototype, individual nodes can play different roles. Among all nodes in the network, a leader is elected. The leader is the only network participant that can create a new block. By delegating operations such as creation of new blocks or leader election only to certain network nodes, we have achieved more effective communication between nodes and nodes are able to reach consensus faster. The consensus protocol used is based on the Raft consensus protocol

## Build

Clone the repo

```sh
git clone https://github.com/nikiviro/RaftBlockChain.git
cd RaftBlockChain/
```

Build in release mode

```sh
cargo build --release
```

This will produce an executable in the `./target/release` directory.

## Setup

### Using Docker

RaftBlockChain supports the use of Docker to provide an easy installation process by providing a single package that gives the user everything he
needs to get node up and running. In order to get the installation package, run the following command after installing Docker:

```sh
docker build -t blockchain_docker_image .
```    

Before the first start of the node it is necessary to:
- generate the public and private key of the node which must be inserted into the configuration file, see Generation of private and public key
- create config files - config.json and genesis.json, you can see example config files in the config/ folder 

The Docker container is set up to use files with these names (config.json, genesis.json) by default when running the blockchain node. 
These files need to be modified based on the required network configuration.

If you want to run the blockchain node, run the following command:
```sh
docker run -p 4001:4001 -v <path to folder with config files>:/config blockchain_docker_image
```

This should result in RaftBlockChain node running.

#### Generation of public and private key

To generate public and private key run: 

```sh
docker run blockchain_docker_image --generate_keypair
```  
The generated private and public key is written to the console. The private and public keys must be entered in the configuration file (config/config.json).