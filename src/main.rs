use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::time::Instant;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Msg {
    src: String,
    dest: String,
    body: Body,
}

impl Msg {
    fn send(self, wtr: &mut impl Write) -> Result<()> {
        serde_json::to_writer(&mut *wtr, &self)?;
        wtr.write_all(b"\n")?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Body {
    #[serde(flatten)]
    pl: Pl,
    #[serde(skip_serializing_if = "Option::is_none")]
    msg_id: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_reply_to: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        messages: Option<Vec<usize>>,
        #[serde(skip_serializing_if = "Option::is_none")]
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

impl TryFrom<String> for Msg {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        serde_json::from_str(&value).context("deserialize from text")
    }
}

fn proc(
    bufrdr: impl BufRead,
    wtr: &mut impl Write,
    node_id: &mut String,
    counter: &mut usize,
) -> Result<()> {
    let mut messages = Vec::new();
    let mut t = HashMap::new();
    for line in bufrdr.lines() {
        let line = line?;
        dbg!(&line);
        let req: Msg = line.try_into()?;
        match req.body.pl {
            Pl::Init { node_id: nid, .. } => {
                *node_id = nid;
                dbg!(&node_id);
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::InitOk,
                        in_reply_to: req.body.msg_id,
                        msg_id: None,
                    },
                };
                resp.send(wtr)?;
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
                resp.send(wtr)?;
            }
            Pl::Generate => {
                let mut f = File::open("/dev/urandom")?;
                let mut buf = [0; 16];
                f.read_exact(&mut buf)?;
                let now = Instant::now();
                let id = format!("{node_id}-{buf:?}-{counter}-{now:?}");
                dbg!(&id);
                *counter += 1;
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::GenerateOk { id },
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(wtr)?;
            }
            Pl::Topology { topology } => {
                t = topology;
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::TopologyOk,
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(wtr)?;
            }
            Pl::Broadcast { message } => {
                let resp = Msg {
                    src: req.dest.clone(),
                    dest: req.src.clone(),
                    body: Body {
                        pl: Pl::BroadcastOk,
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(wtr)?;
                // naive solution for multi node broadcaast, i will send all messages to all nodes in topology
                // should be hash set
                if messages.iter().find(|&&x| x == message).is_none() {
                    messages.push(message);
                    let node_topo = t
                        .get(node_id)
                        .expect("we expect to have a valid topology being passed at init");
                    dbg!(&node_topo);
                    for send_to_node in node_topo {
                        // let's see if i can reuse Broadcast for this purpose or i need to create
                        // another method
                        dbg!(message);
                        let resp = Msg {
                            src: node_id.clone(),
                            dest: send_to_node.to_string(),
                            body: Body {
                                pl: Pl::Broadcast { message },
                                msg_id: None,
                                in_reply_to: None,
                            },
                        };
                        resp.send(wtr)?;
                    }
                }
            }
            // since i reuse Broadcast to spread the message, i have also to respond to cast ok!
            Pl::BroadcastOk => {
                dbg!("internal broadcast from node to node was received");
            }
            Pl::Read => {
                let mut m = None;
                if !messages.is_empty() {
                    m = Some(messages.clone());
                }
                let v = None;
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::ReadOk {
                            messages: Some(messages.clone()),
                            value: v,
                        },
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(wtr)?;
            }
            Pl::Add { delta } => {
                dbg!(delta);
                let resp = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::AddOk,
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                resp.send(wtr)?;
            }
            _ => todo!(),
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    dbg!("node start");
    let mut counter = 0;
    let mut node_id = String::new();
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    proc(stdin, &mut stdout, &mut node_id, &mut counter)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let mut counter = 0;
        let mut node_id = String::new();
        let input = io::Cursor::new(
            r#"{"src":"c0","dest":"n0","body":{"type":"init","msg_id":1,"node_id":"n3","node_ids":["n1","n2","n3"]}}"#,
        );
        let mut output = Vec::new();

        proc(input, &mut output, &mut node_id, &mut counter).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap().trim(),
            r#"{"src":"n0","dest":"c0","body":{"type":"init_ok","in_reply_to":1}}"#
        );
    }

    #[test]
    fn test_echo() {
        let mut counter = 0;
        let mut node_id = String::new();
        let input = io::Cursor::new(
            r#"{"src":"c1","dest":"n1","body":{"type":"echo","msg_id":1,"echo":"Please echo 35"}}"#,
        );
        let mut output = Vec::new();

        proc(input, &mut output, &mut node_id, &mut counter).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap().trim(),
            r#"{"src":"n1","dest":"c1","body":{"type":"echo_ok","echo":"Please echo 35","msg_id":1,"in_reply_to":1}}"#
        );
    }
}
