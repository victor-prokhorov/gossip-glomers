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
