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
    dest: String,
    body: Body,
}

impl Msg {
    fn into_resp(self, id: &mut usize) -> Msg {
        let msg_id = *id;
        *id += 1;
        Msg {
            src: self.dest,
            dest: self.src,
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
        message: usize,
    },
    BroadcastOk,
    Read,
    ReadOk {
        messages: Option<HashSet<usize>>,
    },
    Topology {
        topology: HashMap<String, Vec<String>>,
    },
    TopologyOk,
    Gossip {
        messages: HashSet<usize>,
    },
    Ping,
    PingOk,
    Add {
        delta: usize,
    },
    AddOk,
}

enum InternalMsg {
    Gossip,
    Ping,
}

enum Evt {
    Client(Msg),
    Server(InternalMsg),
}

fn main() -> Result<()> {
    let mut id = String::new();
    let mut ids = Vec::new();
    let mut msg_id = 0;
    let mut stdout = io::stdout().lock();
    let (txc, rx) = sync::mpsc::channel();
    let txs = txc.clone();
    let mut seen = HashSet::new();
    let mut neighbourhood: Vec<String> = Vec::new();
    let jhc = thread::spawn(move || {
        let stdin = io::stdin().lock();
        for line in stdin.lines() {
            let line = line?;
            let req: Msg = serde_json::from_str(&line)?;
            let evt = Evt::Client(req);
            txc.send(evt)?;
        }
        Ok::<_, Error>(())
    });
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(100));
        if let Err(_) = txs.send(Evt::Server(InternalMsg::Ping)) {
            break;
        };
    });
    for evt in rx {
        match evt {
            Evt::Client(msg) => {
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
                        neighbourhood = if id == *ids.first().unwrap() {
                            ids.iter()
                                .filter(|id| *id != ids.first().unwrap())
                                .cloned()
                                .collect()
                        } else {
                            vec![ids.first().unwrap().to_string()]
                        };
                        resp.body.pl = Pl::TopologyOk;
                        resp.send(&mut stdout)?;
                    }
                    Pl::Broadcast { message } => {
                        seen.insert(message);
                        resp.body.pl = Pl::BroadcastOk;
                        resp.send(&mut stdout)?;
                    }
                    Pl::Gossip { messages } => {
                        seen.extend(messages);
                    }
                    Pl::Ping => {
                        resp.body.pl = Pl::PingOk;
                        resp.send(&mut stdout)?;
                    }
                    Pl::PingOk => {
                        for node_id in &neighbourhood {
                            let resp = Msg {
                                src: id.clone(),
                                dest: node_id.clone(),
                                body: Body {
                                    pl: Pl::Gossip {
                                        messages: seen.clone(),
                                    },
                                    msg_id: None,
                                    in_reply_to: None,
                                },
                            };
                            resp.send(&mut stdout)?;
                        }
                    }
                    Pl::Read => {
                        resp.body.pl = Pl::ReadOk {
                            messages: Some(seen.clone()),
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
            Evt::Server(msg) => match msg {
                InternalMsg::Gossip => {
                    for node_id in &neighbourhood {
                        let resp = Msg {
                            src: id.clone(),
                            dest: node_id.clone(),
                            body: Body {
                                pl: Pl::Gossip {
                                    messages: seen.clone(),
                                },
                                msg_id: None,
                                in_reply_to: None,
                            },
                        };
                        resp.send(&mut stdout)?;
                    }
                }
                InternalMsg::Ping => {
                    for node_id in &neighbourhood {
                        let resp = Msg {
                            src: id.clone(),
                            dest: node_id.clone(),
                            body: Body {
                                pl: Pl::Ping,
                                msg_id: None,
                                in_reply_to: None,
                            },
                        };
                        resp.send(&mut stdout)?;
                    }
                }
            },
        }
    }
    jhc.join().unwrap()?;
    Ok(())
}
