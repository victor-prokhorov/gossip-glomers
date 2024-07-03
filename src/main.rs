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
        #[serde(rename = "messages")]
        msgs: Option<HashSet<usize>>,
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
    Add {
        delta: usize,
    },
    AddOk,
}

enum Task {
    Gossip,
}

enum Evt {
    Ext(Msg),
    Int(Task),
}

fn main() -> Result<()> {
    let mut id = String::new();
    let mut ids = Vec::new();
    let mut msg_id = 0;
    let mut stdout = io::stdout().lock();
    let (txc, rx) = sync::mpsc::channel();
    let txs = txc.clone();
    let mut messages = HashSet::new();
    let mut seen = HashMap::new();
    let mut neighbourhood = Vec::new();
    let mut pending = HashMap::new();
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
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(100));
        if txs.send(Evt::Int(Task::Gossip)).is_err() {
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
                    Pl::Topology { .. } => {
                        let central = ids.first().unwrap().clone();
                        neighbourhood = if id == *central {
                            ids.iter().filter(|id| **id != central).cloned().collect()
                        } else {
                            vec![central]
                        };
                        seen = ids.iter().map(|id| (id.clone(), HashSet::new())).collect();
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
                    Pl::GossipOk { id } => {
                        if let Some(pl) = pending.remove(&id) {
                            seen.get_mut(&resp.dst).unwrap().extend(pl);
                        }
                    }
                    Pl::Read => {
                        resp.body.pl = Pl::ReadOk {
                            msgs: Some(messages.clone()),
                        };
                        resp.send(&mut stdout)?;
                    }
                    Pl::Add { .. } => {
                        resp.body.pl = Pl::AddOk;
                        resp.send(&mut stdout)?;
                    }
                    _ => panic!(),
                };
            }
            Evt::Int(task) => match task {
                Task::Gossip => {
                    for host in &neighbourhood {
                        // todo: infinitely growing when the node is able to recv but not send
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
                            eprintln!("{}/{}", unseen_by_host.len(), messages.len());
                            resp.send(&mut stdout)?;
                            // todo: same issue
                            pending.insert(msg_id, unseen_by_host.clone());
                            msg_id += 1;
                        }
                    }
                }
            },
        }
    }
    jhc.join().unwrap()?;
    Ok(())
}
