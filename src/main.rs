use anyhow::Error;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::io::BufRead;
use std::io::Write;
use std::sync;
use std::thread;
use std::time::Duration;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Msg {
    src: String,
    #[serde(rename = "dest")]
    dst: String,
    body: Body,
}

impl Msg {
    fn into_resp(self, id: &mut usize) -> Msg {
        let msg_id = *id;
        *id += 1;
        Msg {
            src: self.dst,
            dst: self.src,
            body: Body {
                pl: self.body.pl,
                msg_id: Some(msg_id),
                in_reply_to: self.body.msg_id,
            },
        }
    }

    fn send(self, stdout: &mut impl Write) -> Result<()> {
        serde_json::to_writer(&mut *stdout, &self)?;
        stdout.write_all(b"\n")?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Body {
    #[serde(flatten)]
    pl: Pl,
    msg_id: Option<usize>,
    in_reply_to: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum Pl {
    Txn {
        txn: Vec<TxnOp>,
    },
    TxnOk {
        txn: Vec<TxnOp>,
    },
    BroadcastTxn {
        txns: Vec<SeqTxn>,
    },
    Error {
        code: usize,
        text: String,
    },
    Init {
        node_id: String,
        node_ids: Vec<String>,
    },
    InitOk,
    Echo {
        echo: String,
    },
    EchoOk {
        echo: String,
    },
    Generate,
    GenerateOk {
        id: String,
    },
    Broadcast {
        #[serde(rename = "message")]
        msg: usize,
    },
    BroadcastOk,
    // read is used to read from the node, but it's also used in lin-kv!
    // by default it's just read and for lin-kv we have fields
    Read {
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        msg_id: Option<usize>,
    },
    ReadOk {
        #[serde(rename = "messages", skip_serializing_if = "Option::is_none")]
        msgs: Option<HashSet<usize>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<usize>,
    },
    Topology {
        topology: HashMap<String, Vec<String>>,
    },
    TopologyOk,
    Gossip {
        #[serde(rename = "messages")]
        msgs: HashSet<usize>,
    },
    GossipOk {
        id: usize,
    },
    GossipCntr {
        cntr: usize,
    },
    Add {
        delta: usize,
    },
    AddOk,
    Send {
        key: String,
        msg: usize,
    },
    SendMany {
        key: String,
        msgs: Vec<usize>,
    },
    SendOk {
        offset: usize,
    },
    Poll {
        offsets: HashMap<String, usize>,
    },
    PollOk {
        msgs: HashMap<String, Vec<(usize, usize)>>,
    },
    CommitOffsets {
        offsets: HashMap<String, usize>,
    },
    CommitOffsetsOk,
    ListCommittedOffsets {
        keys: Vec<String>,
    },
    ListCommittedOffsetsOk {
        offsets: HashMap<String, usize>,
    },
}

enum Task {
    CentralGossip,
    MeshGossip,
    GossipCntr,
    SwitchPhase,
}

enum Evt {
    Ext(Msg),
    Int(Task),
}

type TxnOp = (char, usize, Option<usize>);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeqTxn {
    seq: usize,
    txn: Vec<TxnOp>,
}

struct State {
    txn_id: usize,
    cur_epoch: usize,
    receiving: bool,
}

impl State {
    fn new() -> Self {
        Self {
            txn_id: 0,
            cur_epoch: 0,
            receiving: true,
        }
    }

    fn next_txn_id(&mut self) -> usize {
        let id = self.txn_id;
        self.txn_id += 1;
        id
    }

    fn switch_phase(&mut self) {
        self.receiving = !self.receiving;
        eprintln!("receiving = {}", self.receiving);
        if !self.receiving {
            self.cur_epoch += 1;
        }
    }
}

struct Broadcaster {
    broadcast_nodes: HashMap<usize, HashMap<String, Vec<SeqTxn>>>,
    neighborhood: Vec<String>,
}

impl Broadcaster {
    fn new() -> Self {
        Self {
            broadcast_nodes: HashMap::new(),
            neighborhood: Vec::new(),
        }
    }

    fn init(&mut self, from_ids: Vec<String>) {
        self.neighborhood = from_ids;
    }

    fn push(&mut self, timestamp: usize, nodeid: String, txns: Vec<SeqTxn>) {
        self.broadcast_nodes
            .entry(timestamp)
            .or_default()
            .insert(nodeid, txns);
    }

    fn has_all(&self, epoch: usize) -> bool {
        let epoch_broadcasts = self.broadcast_nodes.get(&epoch);
        if epoch_broadcasts.is_none() {
            return self.neighborhood.is_empty();
        }
        epoch_broadcasts.unwrap().len() == self.neighborhood.len()
    }

    fn get_all(&self, epoch: usize) -> Vec<SeqTxn> {
        self.broadcast_nodes
            .get(&epoch)
            .unwrap_or(&HashMap::new())
            .values()
            .flatten()
            .cloned()
            .collect()
    }
}

struct KVStore {
    kv: HashMap<usize, usize>,
}

impl KVStore {
    fn new() -> Self {
        Self { kv: HashMap::new() }
    }

    fn apply_transaction(&mut self, txn: &[TxnOp]) -> Vec<TxnOp> {
        txn.iter()
            .map(|&(op, key, value)| match op {
                'r' => (op, key, self.kv.get(&key).copied()),
                'w' => {
                    self.kv
                        .insert(key, value.expect("writes MUST contain a value"));
                    (op, key, value)
                }
                _ => panic!(),
            })
            .collect()
    }
}

fn main() -> Result<()> {
    let mut state = State::new();
    let mut broadcaster = Broadcaster::new();
    let mut kvstore = KVStore::new();
    // find better way of constructing state
    // some of those value are never null but some are optionoal
    // in this structure it's not clear which one is which
    let mut id = String::new();
    let mut ids = Vec::new();
    let mut msg_id = 0;
    // timestamp
    // let mut ts = 0;
    let mut txn_id = 0; // clock
    let mut stdout = io::stdout().lock();
    let mut cntr = 0;
    let mut cntrs = HashMap::new();
    let (txc, rx) = sync::mpsc::channel();
    let txsc = txc.clone();
    let txsm = txc.clone();
    let mut messages = HashSet::new();
    let mut seen = HashMap::new();
    let mut default_neighbourhood = Vec::new();
    let mut central_neighbourhood = Vec::new();
    let mut leader = String::new();
    let mut mesh_neighbourhood: Vec<String> = Vec::new();
    let mut pending = HashMap::new();
    // msgs by key
    let mut logs: HashMap<String, Vec<usize>> = HashMap::new();
    // offset by key
    let mut committed_offsets: HashMap<String, usize> = HashMap::new();
    let jhc = thread::spawn(move || {
        let stdin = io::stdin().lock();
        for line in stdin.lines() {
            let line = line?;
            let req: Msg = serde_json::from_str(&line)?;
            let evt = Evt::Ext(req);
            txc.send(evt)?;
        }
        Ok::<_, Error>(())
    });
    // split into lib and bin per challenge
    // lib should probably have `State` struct that is impl by bin
    #[cfg(feature = "broadcast")]
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(1000));
        if txsc.send(Evt::Int(Task::CentralGossip)).is_err() {
            break;
        };
    });
    #[cfg(feature = "broadcast")]
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(300));
        if txsm.send(Evt::Int(Task::MeshGossip)).is_err() {
            break;
        };
    });
    #[cfg(feature = "g-counter")]
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(10));
        if txsm.send(Evt::Int(Task::GossipCntr)).is_err() {
            break;
        };
    });
    #[cfg(feature = "totally")]
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        if txsc.send(Evt::Int(Task::SwitchPhase)).is_err() {
            break;
        };
    });

    for evt in rx {
        match evt {
            Evt::Ext(msg) => {
                let mut resp = msg.into_resp(&mut msg_id);
                match resp.body.pl {
                    Pl::Error { code, text } => {
                        eprintln!("===error===");
                        match code {
                            30 => {
                                eprintln!("err:{}\nfrom:{}", text, resp.dst);
                            }
                            _ => {
                                dbg!(code, &text);
                            }
                        }
                    }
                    Pl::Init { node_id, node_ids } => {
                        id = node_id.clone();
                        ids = node_ids.clone();
                        let central = ids.first().unwrap().clone();
                        central_neighbourhood = if id == *central {
                            ids.iter().filter(|x| **x != central).cloned().collect()
                        } else {
                            vec![central.clone()]
                        };
                        leader = central;
                        mesh_neighbourhood = ids.iter().filter(|x| **x != id).cloned().collect();
                        // self is included but never used
                        seen = ids.iter().map(|id| (id.clone(), HashSet::new())).collect();
                        // self included but equal 0
                        cntrs = ids.iter().map(|id| (id.clone(), 0)).collect();
                        resp.body.pl = Pl::InitOk;
                        resp.send(&mut stdout)?;
                        // double check for all those clones after all challenges solved
                        broadcaster.init(mesh_neighbourhood.clone());
                    }
                    Pl::Txn { txn } => {
                        let txn_id = state.next_txn_id();
                        let result = kvstore.apply_transaction(&txn);
                        broadcaster.push(
                            txn_id,
                            id.clone(),
                            vec![SeqTxn {
                                seq: txn_id,
                                txn: result.clone(),
                            }],
                        );
                        resp.body.pl = Pl::TxnOk { txn: result };
                        resp.send(&mut stdout)?;
                    }
                    Pl::BroadcastTxn { txns } => {
                        eprintln!("broadcast txn recv of len {}", txns.len());
                        broadcaster.push(state.txn_id, resp.dst.clone(), txns);
                    }
                    Pl::Echo { echo } => {
                        resp.body.pl = Pl::EchoOk { echo };
                        resp.send(&mut stdout)?;
                    }
                    Pl::Generate => {
                        resp.body.pl = Pl::GenerateOk {
                            id: Uuid::now_v7().to_string(),
                        };
                        resp.send(&mut stdout)?;
                    }
                    Pl::Topology { topology } => {
                        default_neighbourhood = topology[&id].clone();
                        resp.body.pl = Pl::TopologyOk;
                        resp.send(&mut stdout)?;
                    }
                    Pl::Broadcast { msg } => {
                        messages.insert(msg);
                        resp.body.pl = Pl::BroadcastOk;
                        resp.send(&mut stdout)?;
                    }
                    Pl::Gossip { msgs } => {
                        messages.extend(msgs.clone());
                        seen.get_mut(&resp.dst).unwrap().extend(msgs.clone());
                        resp.body.pl = Pl::GossipOk {
                            id: resp.body.in_reply_to.unwrap(),
                        };
                        resp.send(&mut stdout)?;
                    }
                    Pl::GossipCntr { cntr } => {
                        // or default is not really needed since i did init all ot them with 0
                        *cntrs.entry(resp.dst).or_default() = cntr;
                    }
                    Pl::GossipOk { id } => {
                        if let Some(pl) = pending.remove(&id) {
                            seen.get_mut(&resp.dst).unwrap().extend(pl);
                        }
                    }
                    Pl::Read { key, msg_id } => {
                        if key.is_some() || msg_id.is_some() {
                            panic!("key is suposed to be recvd ONLY by lin-kv so nodes should never see this value");
                        }
                        eprintln!("readp pl");
                        resp.body.pl = Pl::ReadOk {
                            msgs: if messages.is_empty() {
                                None
                            } else {
                                Some(messages.clone())
                            },
                            #[cfg(feature = "g-counter")]
                            value: Some(cntrs.values().sum::<usize>() + cntr),
                            #[cfg(not(feature = "g-counter"))]
                            value: None,
                        };
                        resp.send(&mut stdout)?;
                    }
                    Pl::Add { delta } => {
                        cntr += delta;
                        resp.body.pl = Pl::AddOk;
                        resp.send(&mut stdout)?;
                    }
                    // write, redirect to leader
                    Pl::Send { key, msg } => {
                        // this will probably fail, if leader is partioned the writes would be lost
                        // either use lin-kv either send msgs of confirmations which might become slow
                        if id == leader {
                            let msgs = logs.entry(key.clone()).or_default();
                            // naively relying on unique msgs
                            if !msgs.contains(&msg) {
                                msgs.push(msg);
                            }
                            resp.body.pl = Pl::SendOk {
                                offset: logs[&key].len() - 1,
                            };
                            resp.send(&mut stdout)?; // respond to the req, but now spread the update
                                                     // ok so just to validate, i will send all msgs, which is super slow
                                                     // ideally:
                                                     // 1. we send a vector
                                                     // 2. leader have info on which last msg was
                                                     //    seen, if not fallback to all
                            for x in &central_neighbourhood {
                                let msg_to_replica = Msg {
                                    src: id.clone(),
                                    dst: x.clone(),
                                    body: Body {
                                        pl: Pl::SendMany {
                                            key: key.clone(),
                                            msgs: logs[&key].clone(),
                                        },
                                        msg_id: None,
                                        in_reply_to: None,
                                    },
                                };
                                msg_to_replica.send(&mut stdout)?;
                            }
                        } else {
                            // this node is a replica and shouls send the write pl to leader
                            resp.dst = leader.clone();
                            resp.body.pl = Pl::Send { key, msg };
                            resp.send(&mut stdout)?;
                        }
                    }
                    Pl::SendMany { key, msgs } => {
                        let v = logs.get(&key);
                        if v.is_some() && v.unwrap().len() > msgs.len() {
                        } else {
                            logs.insert(key, msgs);
                        }
                    }
                    // read
                    Pl::Poll { offsets } => {
                        resp.body.pl = Pl::PollOk {
                            msgs: offsets
                                .into_iter()
                                .filter_map(|(key, offset)| {
                                    logs.get(&key).map(|msgs| {
                                        (
                                            key,
                                            msgs.iter()
                                                .enumerate()
                                                .filter(|(i, _)| *i >= offset)
                                                .map(|(i, msg)| (i, *msg))
                                                .collect(),
                                        )
                                    })
                                })
                                .collect(),
                        };
                        resp.send(&mut stdout)?;
                    }
                    // redirect to leader
                    Pl::CommitOffsets { offsets } => {
                        if id == leader {
                            for (key, offset) in &offsets {
                                committed_offsets
                                    .entry(key.to_string())
                                    .and_modify(|x| *x = (*x).max(*offset))
                                    .or_insert(*offset);
                            }
                            resp.body.pl = Pl::CommitOffsetsOk;
                            resp.send(&mut stdout)?;
                            for x in &central_neighbourhood {
                                let msg_to_replic = Msg {
                                    src: id.clone(),
                                    dst: x.clone(),
                                    body: Body {
                                        pl: Pl::CommitOffsets {
                                            offsets: offsets.clone(),
                                        },
                                        msg_id: None,
                                        in_reply_to: None,
                                    },
                                };
                                msg_to_replic.send(&mut stdout);
                            }
                        } else {
                            // this node is a replica and shouls send the write pl to leader
                            resp.dst = leader.clone();
                            resp.body.pl = Pl::CommitOffsets { offsets };
                            resp.send(&mut stdout)?;
                        }
                    }
                    // serve from replicas
                    Pl::ListCommittedOffsets { keys } => {
                        resp.body.pl = Pl::ListCommittedOffsetsOk {
                            offsets: keys
                                .into_iter()
                                .filter_map(|x| {
                                    committed_offsets.get(&x).map(|offset| (x, *offset))
                                })
                                .collect(), // offsets: committed_offsets.iter().map(||{ }).collect(),
                        };
                        resp.send(&mut stdout)?;
                    }
                    Pl::AddOk
                    | Pl::InitOk
                    | Pl::EchoOk { .. }
                    | Pl::GenerateOk { .. }
                    | Pl::BroadcastOk
                    | Pl::ReadOk { .. }
                    | Pl::TopologyOk
                    | Pl::SendOk { .. }
                    | Pl::TxnOk { .. }
                    | Pl::PollOk { .. }
                    | Pl::CommitOffsetsOk { .. }
                    | Pl::ListCommittedOffsetsOk { .. } => panic!("nope"),
                };
            }
            Evt::Int(task) => match task {
                Task::SwitchPhase => {
                    state.switch_phase();
                    if !state.receiving && broadcaster.has_all(state.cur_epoch) {
                        let all_txns = broadcaster.get_all(state.cur_epoch);
                        eprintln!("about to send all txns of len {}", all_txns.len());
                        for txn in &all_txns {
                            kvstore.apply_transaction(&txn.txn);
                        }
                        for node in &broadcaster.neighborhood {
                            let broadcast_msg = Msg {
                                src: id.clone(),
                                dst: node.clone(),
                                body: Body {
                                    pl: Pl::BroadcastTxn {
                                        txns: all_txns.clone(),
                                    },
                                    msg_id: Some(msg_id),
                                    in_reply_to: None,
                                },
                            };
                            broadcast_msg.send(&mut stdout)?;
                            msg_id += 1;
                        }
                    }
                }
                Task::CentralGossip => {
                    for host in &central_neighbourhood {
                        // one day check ever growing when particioned
                        let unseen_by_host: HashSet<_> =
                            messages.difference(&seen[host]).copied().collect();
                        if !unseen_by_host.is_empty() {
                            let resp = Msg {
                                src: id.clone(),
                                dst: host.clone(),
                                body: Body {
                                    pl: Pl::Gossip {
                                        msgs: unseen_by_host.clone(),
                                    },
                                    msg_id: Some(msg_id),
                                    in_reply_to: None,
                                },
                            };
                            resp.send(&mut stdout)?;
                            pending.insert(msg_id, unseen_by_host.clone());
                            msg_id += 1;
                        }
                    }
                }
                Task::MeshGossip => {
                    for host in &mesh_neighbourhood {
                        let unseen_by_host: HashSet<_> =
                            messages.difference(&seen[host]).copied().collect();
                        if !unseen_by_host.is_empty() {
                            let resp = Msg {
                                src: id.clone(),
                                dst: host.clone(),
                                body: Body {
                                    pl: Pl::Gossip {
                                        msgs: unseen_by_host.clone(),
                                    },
                                    msg_id: Some(msg_id),
                                    in_reply_to: None,
                                },
                            };
                            resp.send(&mut stdout)?;
                            pending.insert(msg_id, unseen_by_host.clone());
                            msg_id += 1;
                        }
                    }
                }
                Task::GossipCntr => {
                    for node_to_contact in &mesh_neighbourhood {
                        let resp = Msg {
                            src: id.clone(),
                            dst: node_to_contact.clone(),
                            body: Body {
                                pl: Pl::GossipCntr { cntr },
                                msg_id: Some(msg_id),
                                in_reply_to: None,
                            },
                        };
                        resp.send(&mut stdout)?;
                        msg_id += 1;
                    }
                }
            },
        }
    }
    // join
    Ok(())
}
