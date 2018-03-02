extern crate irc;
extern crate dotenv;

use irc::client::prelude::*;

use std::process::{Command as SysCommand, Stdio};
use std::io::{Write, BufReader, BufRead};
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{Ordering, AtomicBool};
use std::env;

fn main() {
  dotenv::dotenv().ok();

  let mut config = Config::default();
  config.nickname = Some("ec2_shell".to_string());
  config.realname = Some("ec2_shell".to_string());
  config.username = Some("ec2_shell".to_string());
  config.server = Some("irc.esper.net".to_string());
  config.port = Some(6697);
  config.use_ssl = Some(true);

  let client = Arc::new(IrcClient::from_config(config).unwrap());

  let quiet = Arc::new(AtomicBool::default());

  client.identify().unwrap();

  client.for_each_incoming(|m| {
    match m.command {
      Command::Response(Response::RPL_WELCOME, _, _) => {
        client.send_privmsg("NickServ", &format!("IDENTIFY ec2_shell {}", env::var("IRC_SHELL_NS_PASS").unwrap())).unwrap();

        ::std::thread::sleep(std::time::Duration::from_secs(3));

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
              quiet.store(!quiet.load(Ordering::Relaxed), Ordering::Relaxed);
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
          let err_client = Arc::clone(&client);
          let err_quiet = Arc::clone(&quiet);
          let err_target = target.clone();
          let stderr = shell.stderr.take().unwrap();
          let handle = ::std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
              if !err_quiet.load(Ordering::Relaxed) {
                err_client.send_privmsg(&err_target, &line.unwrap()).unwrap();
              }
            }
          });
          let stdout = BufReader::new(shell.stdout.take().unwrap());
          for line in stdout.lines() {
            if !quiet.load(Ordering::Relaxed) {
              client.send_privmsg(&target, &line.unwrap()).unwrap();
            }
          }
          handle.join().unwrap();
          shell.wait().unwrap();
        });
      }
      _ => {}
    }
  }).unwrap();
}
