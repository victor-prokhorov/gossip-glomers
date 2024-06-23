use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::io::BufRead;
use std::io::Write;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Msg {
    src: String,
    dest: String,
    body: Body,
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
}

fn proc(bufrdr: impl BufRead, wtr: &mut impl Write) -> Result<()> {
    for line in bufrdr.lines() {
        let line = line?;
        let req: Msg = serde_json::from_str(&line)?;
        match req.body.pl {
            Pl::Init { .. } => {
                let res = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::InitOk,
                        in_reply_to: req.body.msg_id,
                        msg_id: None,
                    },
                };
                serde_json::to_writer(&mut *wtr, &res)?;
                wtr.write_all(b"\n")?;
            }
            Pl::Echo { echo } => {
                let res = Msg {
                    src: req.dest,
                    dest: req.src,
                    body: Body {
                        pl: Pl::EchoOk { echo },
                        msg_id: req.body.msg_id,
                        in_reply_to: req.body.msg_id,
                    },
                };
                serde_json::to_writer(&mut *wtr, &res)?;
                wtr.write_all(b"\n")?;
            }
            _ => todo!(),
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    proc(stdin, &mut stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        let input = io::Cursor::new(
            r#"{"src":"c0","dest":"n0","body":{"type":"init","msg_id":1,"node_id":"n3","node_ids":["n1","n2","n3"]}}"#,
        );
        let mut output = Vec::new();

        proc(input, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap().trim(),
            r#"{"src":"n0","dest":"c0","body":{"type":"init_ok","in_reply_to":1}}"#
        );
    }

    #[test]
    fn test_echo() {
        let input = io::Cursor::new(
            r#"{"src":"c1","dest":"n1","body":{"type":"echo","msg_id":1,"echo":"Please echo 35"}}"#,
        );
        let mut output = Vec::new();

        proc(input, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap().trim(),
            r#"{"src":"n1","dest":"c1","body":{"type":"echo_ok","echo":"Please echo 35","msg_id":1,"in_reply_to":1}}"#
        );
    }
}
