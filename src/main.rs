extern crate irc;
extern crate dotenv;

use irc::client::prelude::*;

use std::process::{Command as SysCommand, Stdio};
use std::io::{Read, Write, BufReader, BufRead};
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{Ordering, AtomicBool, AtomicUsize};
use std::env;

const MAX_RESPONSE: usize = 30;

fn main() {
  dotenv::dotenv().ok();

  let config = Config {
    nickname: Some("ec2_shell".into()),
    realname: Some("ec2_shell".into()),
    username: Some("ec2_shell".into()),
    server: Some("irc.esper.net".into()),
    port: Some(6697),
    use_ssl: Some(true),
    .. Default::default()
  };

  let client = Arc::new(IrcClient::from_config(config).unwrap());

  let quiet = Arc::new(AtomicBool::default());

  client.identify().unwrap();

  client.for_each_incoming(|m| {
    match m.command {
      Command::Response(Response::RPL_WELCOME, _, _) => {
        client.send_privmsg("NickServ", &format!("IDENTIFY ec2_shell {}", env::var("IRC_SHELL_NS_PASS").unwrap())).unwrap();

        thread::sleep(std::time::Duration::from_secs(3));

        client.send_join("#shell").unwrap();
      },
      Command::INVITE(_, chan) => {
        client.send_join(&chan).unwrap();

        let who = m.prefix.unwrap_or_else(|| String::from("someone, but I don't know who,"));

        client.send_privmsg(&chan, &format!("{} asked me to join", who)).unwrap();
      },
      Command::PRIVMSG(target, message) => {
        if target != "#shell" {
          return;
        }
        if message.starts_with("$->") {
          let command = message.chars().skip(3).collect::<String>().to_lowercase();
          match command.as_str() {
            "reset" => {
              SysCommand::new("sudo").args(&["docker", "container", "stop", "irc_shell"]).status().ok();
              SysCommand::new("sudo").args(&["docker", "container", "rm", "irc_shell"]).status().ok();
              SysCommand::new("sudo").args(&["docker", "container", "create", "-i", "--name", "irc_shell", "jkcclemens/irc_shell:v2"]).status().ok();
              SysCommand::new("sudo").args(&["docker", "container", "start", "irc_shell"]).status().ok();
              client.send_privmsg(&target, "k").unwrap();
            },
            "stop" => {
              SysCommand::new("sudo").args(&["docker", "container", "stop", "irc_shell"]).status().ok();
              client.send_privmsg(&target, "k").unwrap();
            },
            "start" => {
              SysCommand::new("sudo").args(&["docker", "container", "start", "irc_shell"]).status().ok();
              client.send_privmsg(&target, "k").unwrap();
            },
            "quiet" => {
              quiet.fetch_xor(true, Ordering::Relaxed);
              let message = if quiet.load(Ordering::Relaxed) {
                "k, shutting up"
              } else {
                "k, loud again"
              };
              client.send_privmsg(&target, message).unwrap();
            },
            "quiet?" => {
              let message = if quiet.load(Ordering::Relaxed) {
                "yeah, I'm quiet"
              } else {
                "no, I'm loud"
              };
              client.send_privmsg(&target, message).unwrap();
            },
            "die" => {
              std::process::exit(1);
            },
            _ => {}
          }
          return;
        }
        if !message.starts_with("$ ") {
          return;
        }
        let command: String = message.chars().skip(2).collect();
        let client = Arc::clone(&client);
        let quiet = Arc::clone(&quiet);
        thread::spawn(move || {
          println!("command: {}", command);

          let mut shell = SysCommand::new("sudo")
            .args(&["docker", "exec", "-i", "irc_shell", "/bin/bash", "--login"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

          shell.stdin.take().unwrap().write_all(command.as_bytes()).unwrap();

          let output_lines = Arc::new(AtomicUsize::default());
          let sent_max = Arc::new(AtomicBool::default());

          let readers: Vec<Box<Read + Send>> = vec![
            Box::new(shell.stdout.take().unwrap()),
            Box::new(shell.stderr.take().unwrap())
          ];

          let mut handles = Vec::new();

          for reader in readers {
            let client = Arc::clone(&client);
            let quiet = Arc::clone(&quiet);
            let output_lines = Arc::clone(&output_lines);
            let sent_max = Arc::clone(&sent_max);
            let target = target.clone();
            let handle = thread::spawn(move || {
              for line in BufReader::new(reader).lines() {
                if !quiet.load(Ordering::Relaxed) {
                  if output_lines.load(Ordering::SeqCst) >= MAX_RESPONSE {
                    if !sent_max.load(Ordering::SeqCst) {
                      client.send_privmsg(&target, "Too many lines. Ignoring the rest.").unwrap();
                      sent_max.store(true, Ordering::SeqCst);
                    }
                    continue;
                  }
                  client.send_privmsg(&target, &line.unwrap()).unwrap();
                  output_lines.fetch_add(1, Ordering::SeqCst);
                }
              }
            });
            handles.push(handle);
          };

          for handle in handles {
            handle.join().unwrap();
          }
          let status = shell.wait().unwrap();
          if output_lines.load(Ordering::SeqCst) == 0 {
            if let Some(code) = status.code() {
              client.send_privmsg(&target, &format!("Exited with status code {}.", code)).unwrap();
            }
          }
        });
      }
      _ => {}
    }
  }).unwrap();
}
