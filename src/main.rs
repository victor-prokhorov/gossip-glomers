use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::io::BufRead;
use std::io::Write;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Msg {
    src: String,
    dest: String,
    body: Body,
}

impl Msg {
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
        messages: Option<Vec<usize>>,
        value: Option<usize>,
    },
    Topology {
        topology: HashMap<String, Vec<String>>,
    },
    TopologyOk,
    Add {
        delta: usize,
    },
    AddOk,
}

fn main() -> Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    for line in stdin.lines() {
        let line = line?;
        let req: Msg = serde_json::from_str(&line)?;
        match req.body.pl {
            Pl::Init { .. } => {
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::InitOk,
                        in_reply_to: req.body.msg_id,
                        msg_id: None,
                    },
                };
                resp.send(&mut stdout)?;
            }
            Pl::Echo { echo } => {
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::EchoOk { echo },
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(&mut stdout)?;
            }
            Pl::Generate => {
                let id = Uuid::now_v7();
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::GenerateOk { id: id.to_string() },
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(&mut stdout)?;
            }
            Pl::Topology { .. } => {
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::TopologyOk,
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(&mut stdout)?;
            }
            Pl::Broadcast { .. } => {
                let resp = Msg {
                    src: req.dest.clone(),
                    dest: req.src.clone(),
                    body: Body {
                        pl: Pl::BroadcastOk,
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(&mut stdout)?;
            }
            Pl::Read => {
                let v = None;
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::ReadOk {
                            messages: None,
                            value: v,
                        },
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(&mut stdout)?;
            }
            Pl::Add { .. } => {
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::AddOk,
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(&mut stdout)?;
            }
            _ => panic!(),
        }
    }
    Ok(())
}
