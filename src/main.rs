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

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
struct Body {
    #[serde(flatten)]
    pl: Pl,
    msg_id: Option<usize>,
    in_reply_to: Option<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum Pl {
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
    Read,
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
}

enum Evt {
    Ext(Msg),
    Int(Task),
}

fn main() -> Result<()> {
    // find better way of constructing state
    // some of those value are never null but some are optionoal
    // in this structure it's not clear which one is which
    let mut id = String::new();
    let mut ids = Vec::new();
    let mut msg_id = 0;
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
    let mut mesh_neighbourhood = Vec::new();
    let mut pending = HashMap::new();
    // msgs by key
    let mut logs = HashMap::new();
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
    for evt in rx {
        match evt {
            Evt::Ext(msg) => {
                let mut resp = msg.into_resp(&mut msg_id);
                match resp.body.pl {
                    Pl::Init { node_id, node_ids } => {
                        id = node_id;
                        ids = node_ids;
                        let central = ids.first().unwrap().clone();
                        central_neighbourhood = if id == *central {
                            ids.iter().filter(|x| **x != central).cloned().collect()
                        } else {
                            vec![central]
                        };
                        mesh_neighbourhood = ids.iter().filter(|x| **x != id).cloned().collect();
                        // self is included but never used
                        seen = ids.iter().map(|id| (id.clone(), HashSet::new())).collect();
                        // self included but equal 0
                        cntrs = ids.iter().map(|id| (id.clone(), 0)).collect();
                        resp.body.pl = Pl::InitOk;
                        resp.send(&mut stdout)?;
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
                    Pl::Read => {
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
                    Pl::Send { key, msg } => {
                        logs.entry(key.clone()).or_insert(Vec::new()).push(msg);
                        resp.body.pl = Pl::SendOk {
                            offset: logs[&key].len() - 1,
                        };
                        resp.send(&mut stdout)?;
                    }
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
                                    // better to have logs as map of msgs per key directly!
                                    // (
                                    //     key,
                                    //     logs.iter()
                                    //         .enumerate()
                                    //         .filter(|(i, (_, _))| *i >= offset)
                                    //         .map(|(i, (_, log))| (i, *log))
                                    //         .collect(),
                                    // )
                                })
                                .collect(),
                        };
                        resp.send(&mut stdout)?;
                    }
                    Pl::CommitOffsets { offsets } => {
                        for (key, offset) in offsets {
                            committed_offsets
                                .entry(key)
                                .and_modify(|x| *x = (*x).max(offset))
                                .or_insert(offset);
                        }
                        resp.body.pl = Pl::CommitOffsetsOk;
                        resp.send(&mut stdout)?;
                    }
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
                    | Pl::PollOk { .. }
                    | Pl::CommitOffsetsOk { .. }
                    | Pl::ListCommittedOffsetsOk { .. } => {
                        panic!("client pl recvd by server")
                    }
                };
            }
            Evt::Int(task) => match task {
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
    jhc.join().unwrap()?;
    Ok(())
}
