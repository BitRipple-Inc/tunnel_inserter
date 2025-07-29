use std::collections::HashMap;
use std::fs::File;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixDatagram;

use nix::sys::socket::{setsockopt, sockopt};

use axl::{axl_tunnel_app, TunnelArgs};
use clap::{Arg, ArgAction, Command};

mod forward;
mod sock_utils;
mod udp;

use crate::forward::{forward, PortPair};
use crate::sock_utils::set_cloexec;

/// Configuration for [`TunnelInserter`].
pub struct TunnelInserterConfig {
  pub outside_fd: i32,
  pub control_fd: i32,
  pub local_addr: Ipv4Addr,
  pub remote_addr: Ipv4Addr,
  pub local_ports: Vec<u16>,
  pub remote_ports: Vec<u16>,
  pub stderr_file: Option<String>,
  /// Arguments for the AxlRust component.  Place holders like `{fd0}` will be
  /// substituted with the file descriptors of the sockets created by the
  /// inserter.
  pub axlrust_args: Vec<String>,
}

fn build_tunnel_args(args: &[String]) -> TunnelArgs {
  let matches = Command::new("axl")
    .arg(Arg::new("config").short('c').long("config").num_args(1))
    .arg(
      Arg::new("config-item")
        .short('x')
        .long("config-item")
        .num_args(1)
        .action(ArgAction::Append),
    )
    .arg(Arg::new("log-filter").long("log-filter").num_args(1))
    .arg(Arg::new("log-format").long("log-format").num_args(1))
    .arg(
      Arg::new("log-format-perror")
        .long("log-format-perror")
        .num_args(1),
    )
    .arg(Arg::new("bind").short('b').long("bind").num_args(1))
    .arg(Arg::new("tun-dev").short('t').long("tun-dev").num_args(1))
    .arg(Arg::new("tunnel-block-timeout-ms").short('d').num_args(1))
    .arg(
      Arg::new("tunnel-block-inactivity-timeout-ms")
        .short('D')
        .num_args(1),
    )
    .arg(
      Arg::new("tunnel-ordering-timeout-ms")
        .short('q')
        .num_args(1),
    )
    .get_matches_from(args);

  TunnelArgs {
    config: matches.get_one::<String>("config").cloned(),
    config_item: matches
      .get_many::<String>("config-item")
      .map(|vals| vals.cloned().collect())
      .unwrap_or_default(),
    log_filter: matches.get_one::<String>("log-filter").cloned(),
    log_format: matches.get_one::<String>("log-format").cloned(),
    log_format_perror: matches.get_one::<String>("log-format-perror").cloned(),
    bind: matches.get_one::<String>("bind").cloned(),
    tun_dev: matches.get_one::<String>("tun-dev").cloned(),
    tunnel_block_timeout_ms: matches
      .get_one::<String>("tunnel-block-timeout-ms")
      .cloned(),
    tunnel_block_inactivity_timeout_ms: matches
      .get_one::<String>("tunnel-block-inactivity-timeout-ms")
      .cloned(),
    tunnel_ordering_timeout_ms: matches
      .get_one::<String>("tunnel-ordering-timeout-ms")
      .cloned(),
  }
}

/// Tunnel inserter logic which was previously implemented in `main.rs`.
pub struct TunnelInserter {
  cfg: TunnelInserterConfig,
}

impl TunnelInserter {
  pub fn new(cfg: TunnelInserterConfig) -> Self {
    Self { cfg }
  }

  /// Run the tunnel inserter.  This function blocks until the control pipe is
  /// closed.
  pub fn run(self) -> Result<(), String> {
    let TunnelInserterConfig {
      outside_fd,
      control_fd,
      local_addr,
      remote_addr,
      mut local_ports,
      mut remote_ports,
      stderr_file,
      axlrust_args,
    } = self.cfg;

    if local_ports.len() != remote_ports.len() {
      return Err("Need the same number of --local-port as --remote-port".to_string());
    }

    // Outside sockets coming from lightway.
    let fd_outside = unsafe { UnixDatagram::from_raw_fd(outside_fd) };
    let fd_pipe = File::from(unsafe { OwnedFd::from_raw_fd(control_fd) });
    set_cloexec(outside_fd, true);
    set_cloexec(control_fd, true);
    fd_outside
      .set_nonblocking(true)
      .expect("Failed to make socket nonblocking");

    // Create inter process sockets which will be passed to AxlRust.
    let mut port_pairs: Vec<PortPair> = Vec::new();
    let mut lsocks: Vec<UnixDatagram> = Vec::new();
    let mut rsocks: Vec<UnixDatagram> = Vec::new();
    for (l, r) in local_ports.drain(..).zip(remote_ports.drain(..)) {
      port_pairs.push(PortPair {
        local: l,
        remote: r,
      });
      let (lsock, rsock) = UnixDatagram::pair().unwrap();
      for sock in [&lsock, &rsock] {
        setsockopt(&sock, sockopt::RcvBuf, &2_000_000).expect("Can't set SO_RCVBUF");
        setsockopt(&sock, sockopt::SndBuf, &2_000_000).expect("Can't set SO_SNDBUF");
      }
      lsock
        .set_nonblocking(true)
        .expect("Failed to make socket nonblocking");
      lsocks.push(lsock);
      set_cloexec(rsock.as_raw_fd(), false);
      rsocks.push(rsock);
    }

    // Substitute the file descriptor place holders in the axlrust arguments.
    let argmap: HashMap<String, String> = (0..lsocks.len())
      .map(|j| {
        let fd = rsocks[j].as_raw_fd();
        (format!("{{fd{j}}}"), format!("{fd}"))
      })
      .collect();
    let args_interp: Vec<String> = axlrust_args
      .iter()
      .map(|s| {
        let mut sr = s.clone();
        for (k, v) in &argmap {
          sr = sr.replace(k, v);
        }
        sr
      })
      .collect();

    // Optional stderr redirection. We simply log the invocation if a file is provided.
    if let Some(mut f) = stderr_file.and_then(|f| File::create(f).ok()) {
      use std::io::Write;
      let _ = writeln!(f, "AxlRust invoked with args: {:?}", args_interp);
    } else {
      println!("AxlRust invoked with args: {:?}", args_interp);
    }

    // Build tunnel arguments and run the tunnel in a separate thread.
    let tunnel_args = build_tunnel_args(&args_interp);
    let handle = std::thread::spawn(move || {
      axl_tunnel_app(&tunnel_args);
    });

    // Start the forwarding logic.
    forward(
      &fd_outside,
      &fd_pipe,
      local_addr,
      remote_addr,
      &port_pairs,
      &lsocks,
    );

    // Forward loop exited, wait for the AxlRust component to finish.
    handle.join().expect("AxlRust thread panicked");

    Ok(())
  }
}
