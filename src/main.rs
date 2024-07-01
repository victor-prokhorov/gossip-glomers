use anyhow::Error;
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
    let (tx, rx) = std::sync::mpsc::channel();
    let mut id = 0;
    let mut stdout = io::stdout().lock();
    let jh = std::thread::spawn(move || {
        let stdin = std::io::stdin().lock();
        for line in stdin.lines() {
            let line = line?;
            let req: Msg = serde_json::from_str(&line)?;
            tx.send(req)?;
        }
        Ok::<_, Error>(())
    });
    for req in rx {
        let mut resp = req.into_resp(&mut id);
        resp.body.pl = match resp.body.pl {
            Pl::Init { .. } => Pl::InitOk,
            Pl::Echo { echo } => Pl::EchoOk { echo },
            Pl::Generate => Pl::GenerateOk {
                id: Uuid::now_v7().to_string(),
            },
            Pl::Topology { .. } => Pl::TopologyOk,
            Pl::Broadcast { .. } => Pl::BroadcastOk,
            Pl::Read => Pl::ReadOk {
                messages: None,
                value: None,
            },
            Pl::Add { .. } => Pl::AddOk,
            _ => panic!(),
        };
        resp.send(&mut stdout)?;
    }
    jh.join().unwrap()?;
    Ok(())
}
