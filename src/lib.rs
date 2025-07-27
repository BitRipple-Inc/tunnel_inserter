//! Library version of tunnel_inserter.
//!
//! The original crate exposed a binary that was called from other
//! applications.  The binary parsed command line arguments and then
//! started the forwarding logic as well as a subprocess running the
//! BitRipple/Axl tunnel.  This file exposes the same functionality in a
//! library friendly way so that users can create a `TunnelInserter`
//! directly without spawning a separate process.

use std::collections::HashMap;
use std::fs::File;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixDatagram;

use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};
use nix::sys::socket::{setsockopt, sockopt};

mod axlrust;
mod forward;
mod sock_utils;
mod udp;

use crate::axlrust::AxlRust;
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

    // Optional stderr redirection.
    let stderr_handle =
      stderr_file.map(|f| File::create(f).expect("Can't create stderr output file"));

    // Start the AxlRust component in a separate thread.
    let axl = AxlRust::new(args_interp, stderr_handle);
    let handle = axl.spawn();

    // Ignore termination signals so that the caller controls shutdown via the
    // control pipe.
    unsafe {
      sigaction(
        Signal::SIGINT,
        &SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty()),
      )
      .expect("sigaction failed");
    };

    // Close unused remote sockets in this process.
    drop(rsocks);

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
