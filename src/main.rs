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
    fn into_resp(self, id: Option<&mut usize>) -> Msg {
        Msg {
            src: self.dest,
            dest: self.src,
            body: Body {
                pl: self.body.pl,
                msg_id: id.map(|id| {
                    let mid = *id;
                    *id += 1;
                    mid
                }),
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
    let mut id = 0;
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    for line in stdin.lines() {
        let line = line?;
        let req: Msg = serde_json::from_str(&line)?;
        let mut resp = req.into_resp(Some(&mut id));
        match resp.body.pl {
            Pl::Init { .. } => {
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
                resp.body.pl = Pl::TopologyOk;
                resp.send(&mut stdout)?;
            }
            Pl::Broadcast { .. } => {
                resp.body.pl = Pl::BroadcastOk;
                resp.send(&mut stdout)?;
            }
            Pl::Read => {
                resp.body.pl = Pl::ReadOk {
                    messages: None,
                    value: None,
                };
                resp.send(&mut stdout)?;
            }
            Pl::Add { .. } => {
                resp.body.pl = Pl::AddOk;
                resp.send(&mut stdout)?;
            }
            _ => panic!(),
        }
    }
    Ok(())
}
