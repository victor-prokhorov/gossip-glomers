# distributed system challenges solved in rust

- [fly.io gossip glomers distributed system challenges](https://fly.io/dist-sys)
- [to test against maelstrom v0.2.3](https://github.com/jepsen-io/maelstrom/releases/tag/v0.2.3)
- [protocal spec to implement node in rust](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md)


## echo

maelstrom expect a binary, a debug one should do so we just have to run it by pointing to the build file

```sh
cargo build && ~/bin/maelstrom/maelstrom test -w echo --bin target/debug/gossip-glomers --node-count 1 --time-limit 10
```

to debug

```sh
~/bin/maelstrom/maelstrom serve
```
