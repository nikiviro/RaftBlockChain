# RaftBlockChain
The following is example of simple blockchain network using Raft consensus algorithm 
### Architecture
There are two types of nodes - leader and fllowers. In this implementation every nodes running in its own thread and use rust channels to communicate with each other. 
Every node is running RAFT consensus protocol and has its own copy of BlockChain. Ininitialy the first created node will become a Raft leader. 
There is a global proposals queue where all the request are pushed. 

There are two types of proposal:
* Configuration change proposal - changes in the raft cluster (Add new node, remove node)
* Normal - block insertion - request to insert new block into the Blockchain 

Leader is responsible for chceking the global proposals queue for new requests and process them. 

In case of block insertion proposal, the leader insert the new block data into the raft log and try to replicate this log amongst all followers. After majority of nodes confirm replication of log entry, the entry is considered as commited. When the entry is commited, all nodes extracts new block data from raft log and insert the block into their local Blockchain. 

### How to use

Clone the repo and build it to start.

```bash
Cargo run 'num_of_nodes'
```

Set enviroment variable to show debug information such as leader change, heartbeat messages, request vote messages, new election..

Example: 

```bash
$ RUST_LOG=info cargo run 10
```

This will create 10 nodes and each node will be running in its own thread. The first created node will become a Raft leader.
In this example a new block is created every 20 seconds. New blocks are then replicated to all nodes and are inserted into blockchain. After 3. block insertion the leader will stop sending heartbeats messages to followers and new election  will be held. 
