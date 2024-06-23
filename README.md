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
