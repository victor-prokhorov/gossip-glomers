# distributed system challenges solved in rust

- [fly.io gossip glomers distributed system challenges](https://fly.io/dist-sys)
- [to test against maelstrom v0.2.3](https://github.com/jepsen-io/maelstrom/releases/tag/v0.2.3)
- [protocol spec to implement node in rust](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md)


## echo

maelstrom expect a binary, a debug one should do so we just have to run it by pointing to the build file

```sh
cargo build && ~/bin/maelstrom/maelstrom test -w echo --bin target/debug/gossip-glomers --node-count 1 --time-limit 10
```

to debug

```sh
~/bin/maelstrom/maelstrom serve
```

## unique id generation

```sh
cargo watch -w src -s 'clear && cargo build && ~/bin/maelstrom/maelstrom test -w unique-ids --bin target/debug/gossip-glomers --time-limit 30 --rate 1000 --node-count 3 --availability total --nemesis partition'
```

from very far imitating uuid version 7 (timestamp, counter and random)
- i expect the `node_id` to be unique and be passed at init
- `buf` is 16 bytes read from `dev/urandom`
-  opening file on each call is redundant
```rust
                let id = format!("{node_id}-{buf:?}-{counter}-{now:?}");
```

## broadcast

### single node broadcast

```sh
cargo watch -w src -s 'clear && cargo build && ~/bin/maelstrom/maelstrom test -w broadcast --bin target/debug/gossip-glomers --node-count 1 --time-limit 20 --rate 10'
```

### multinode broadcast 

- topology does not changes between tests, so i guess we can be smarer about broadcast
- i did tried to send message (write directly to the stdout) from the thread but, i need to
  wrap the message into mutex which will become slow really fast
- even snedin gthe mesage force me to move node id an did
- i will split the events in server(node to node) and client (client to node)

```sh
cargo watch -w src -s 'clear && cargo build && ~/bin/maelstrom/maelstrom test -w broadcast --bin target/debug/gossip-glomers --node-count 5 --time-limit 20 --rate 10'
```

### fault tolerant multinode broadcast

```sh
cargo watch -w src -s 'clear && cargo build && ~/bin/maelstrom/maelstrom test -w broadcast --bin target/debug/gossip-glomers --node-count 5 --time-limit 20 --rate 10 --nemesis partition'
```

### efficient broadcast
./maelstrom test -w broadcast --bin ~/go/bin/maelstrom-broadcast --node-count 25 --time-limit 20 --rate 100 --latency 100

```sh
cargo watch -w src -s 'clear && cargo build && ~/bin/maelstrom/maelstrom test -w broadcast --bin target/debug/gossip-glomers --node-count 25 --time-limit 20 --rate 100 --latency 100'
```

this is the intial output
```
            :stable-latencies {0 7,
                               0.5 1500,
                               0.95 2096,
                               0.99 2218,
                               1 2298},
```
with star topology we got 
```
            :stable-latencies {0 0,
                               0.5 184,
                               0.95 198,
                               0.99 201,
                               1 293},
```
star topology 6 msgs per op so roughly 14 left out of 20
doing mesh out of 25 gives 120 msgs per op
so very very aproximately `120 / 10`
- either do less often mesh call
- either don't completly connect all nodes

i'm sending the full seen msgs but i guess it could be optimized
## grow only counter
todo: should have been multiple binaries actually
```sh
cargo watch -w src -s 'clear && cargo build && ~/bin/maelstrom/maelstrom test -w g-counter --bin target/debug/gossip-glomers --node-count 3 --rate 100 --time-limit 2 --nemesis partition'
```
### kafka style logging

```sh
cargo watch -w src -s 'clear && cargo build --features kafka && ~/bin/maelstrom/maelstrom test -w kafka --bin target/debug/gossip-glomers --node-count 1 --concurrency 2n --time-limit 2 --rate 1000'
```

<https://bravenewgeek.com/building-a-distributed-log-from-scratch-part-2-data-replication/>

There are a number of ways we can go about replicating the log data. Broadly speaking, we can group the techniques into two different categories: gossip/multicast protocols and consensus protocols. The former includes things like epidemic broadcast trees, bimodal multicast, SWIM, HyParView, and NeEM. These tend to be eventually consistent and/or stochastic. The latter, which I’ve described in more detail here, includes 2PC/3PC, Paxos, Raft, Zab, and chain replication. These tend to favor strong consistency over availability.

consensus based replication

1. designate a leader responsible for sequencing writes
2. replicate to the writes

all replicas vs quorum

try with all replicas and probably for the next challenge port to quorum for perfs

kafka use all replicas

- leader selected
- leader maintains in-sync replica (ISR) - fully caugth, in the same state as leader are ISR
- all reads and writes go trought the leader
- leader writes messages to write-ahead log (WAL) considered "dirty" or uncommmited
- leader commits msg once all replicas are ISR (have written to their own WAL)
- leader maintains high-water mark (HW) = last committed msg in WAL
- when replica fetch it gets in the resp HW, this allows replicas to know when to commit

- only commited messages are exposed to consumers
    - wait until msg is commmited on the leader (+ replicated ISR) = durability
    - wait msg only written (not commited) = mix
    - don't wait = latency

let's if i need dynamic leader election to pass the challege
i guess this time i will not get away implementing the thing from scratch
https://github.com/jepsen-io/maelstrom/blob/main/doc/services.md
https://github.com/jepsen-io/maelstrom/blob/main/doc/workloads.md#workload-lin-kv

articles from readme
https://github.com/mchernyakov/gossip-glomers/tree/master
example of lin kv wrapper in rs
https://github.com/metanivek/gossip-glomers-rs/blob/main/src/maelstrom.rs
someone shared apearetnly some rust impl in rust via crate
https://github.com/nachiketkanore/distributed-systems-challenges/blob/main/multi-node-kafka-style-log/Cargo.toml
jonhoo lib impl
https://github.com/jonhoo/rustengan/blob/main/src/lib.rs

my whole system currently expect always `event -> stdout write`
where `event` is either stdin read or self initiated inetrnal to the node signal
that would end in stdout write
BUT now since i reach third party lin kv service, i will need to do
inside reqeust reponsose cycle, reqeust response (to reach this servie then act on reponse)
but i lock stdin in a thread lol...


```sh
cargo watch -w src -s "clear && cargo build --features totally && ~/bin/maelstrom/maelstrom test -w txn-rw-register --bin './target/debug/gossip-glomers' --node-count 1 --time-limit 20 --rate 1000 --concurrency 2n --consistency-models read-uncommitted --availability total"
```

## 6b

goal: replicate our writes across all nodes while ensuring a Read Uncommitted consistency model
<https://jepsen.io/consistency/models/read-uncommitted>
- direty writes = two transactions modify the same object concurrently before committing
- "read uncommitted can be totally available: in the presence of network partitions"
- "read uncommitted does not impose any real-time constraints."
    - "If process A completes write w, then process B begins a read r, r is not necessarily guaranteed to observe w"
- "Without sacrificing availability, you can also ensure that transactions do not read uncommitted state by choosing the stronger read committed."

- g0 (dirty write) 
    - T1: appends 1 to key x
    - T2: appends 2 to key x
    - T1: appends 3 to key x
    - [1,2,3]
