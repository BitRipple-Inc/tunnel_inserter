use clap::{arg, value_parser};
use std::fs::File;
use std::collections::{HashMap};
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::{UnixDatagram};
use std::process::{Command, Stdio};

use nix::sys::signal::{sigaction, SaFlags, SigSet, Signal, SigAction, SigHandler};
use nix::sys::socket::{setsockopt, sockopt};

use crate::sock_utils::set_cloexec;
use crate::forward::{PortPair, forward};

mod udp;
mod sock_utils;
mod forward;

fn main() -> Result<(), String> {
    // Parse command-line arguments
    let matches = clap::Command::new("tunnel_inserter")
        .about("Forwards raw packets to <-> from lightway into \
            a tool such as bitripple tunnel.")
        .arg(arg!(-o --outside <OUTSIDE_FD> "Socket corresponding to outside")
            .value_parser(value_parser!(i32))
            .required(true))
        .arg(arg!(-c --control <CONTROL_FD> "Control pipe file descriptor")
            .value_parser(value_parser!(i32))
            .required(true))
        .arg(arg!(--"local-addr" <IP> "Local IPv4 address")
            .value_parser(value_parser!(Ipv4Addr))
            .required(true))
        .arg(arg!(--"remote-addr" <IP> "Remote IPv4 address")
            .value_parser(value_parser!(Ipv4Addr))
            .required(true))
		.arg(arg!(--"local-ports" <PORTS> "Local ports (space separated)")
            .value_parser(value_parser!(u16))
            .num_args(1..)
            .required(false))
        .arg(arg!(--"remote-ports" <PORTS> "Remote ports (space separated)")
            .value_parser(value_parser!(u16))
            .num_args(1..)
            .required(false))
        .arg(arg!(--"stderr-file" <FILE> "Destination for stderr")
            .required(false))
        .arg(arg!(<CMD> "Command to call")
            .num_args(1..)
            .required(true))
        .get_matches();

    let fd_outside_i = *matches.get_one::<i32>("outside").unwrap();
    let fd_pipe_i = *matches.get_one::<i32>("control").unwrap();
    let cmd: Vec<String> = matches.get_many::<String>("CMD")
        .unwrap()
        .map(|s| { s.to_string() })
        .collect();
	let local_addr = matches.get_one::<Ipv4Addr>("local-addr");
    let remote_addr = matches.get_one::<Ipv4Addr>("remote-addr");
	let local_ports: Vec<u16> = matches
        .get_many::<u16>("local-ports")
        .map(|ports| ports.cloned().collect())
        .unwrap_or_default();
    let remote_ports: Vec<u16> = matches
        .get_many::<u16>("remote-ports")
        .map(|ports| ports.cloned().collect())
        .unwrap_or_default();
    let stderr : Option<&String> = matches.get_one::<String>("stderr-file");

    let local_addr : Ipv4Addr = match local_addr {
        Some(&x) => x,
        None => return Err(String::from("local address not set")),
    };
    let remote_addr : Ipv4Addr = match remote_addr {
        Some(&x) => x,
        None => return Err(String::from("remote address not set")),
    };

    // Validate
    if local_ports.len() != remote_ports.len() {
        return Err(format!("Need the same number of --local-port as \
            --remote-port"));
    }

	// Debug output
    println!("Settings:");
    println!("fd_outside {}, fd_pipe {}",
        fd_outside_i,
        fd_pipe_i);
    println!("cmd: {:?}", cmd);
    println!("local_addr: {:?}", local_addr);
    println!("remote_addr: {:?}", remote_addr);
    println!("local_ports: {:?}", local_ports);
    println!("remote_ports: {:?}", remote_ports);

    // Create the downward sockets
    let fd_outside = unsafe { UnixDatagram::from_raw_fd(fd_outside_i) };
    let fd_pipe = File::from(unsafe { OwnedFd::from_raw_fd(fd_pipe_i) });
    set_cloexec(fd_outside_i, true);
    set_cloexec(fd_pipe_i, true);
    fd_outside.set_nonblocking(true)
        .expect("Failed to make socket nonblocking");

    // Create the upward sockets
    let mut port_pairs : Vec<PortPair> = vec![];
    let mut lsocks : Vec<UnixDatagram> = vec![];
    let mut rsocks : Vec<UnixDatagram> = vec![];
    for (l, r) in local_ports.into_iter().zip(remote_ports) {
        // Add the port pair
        port_pairs.push(PortPair { local: l, remote: r });

        // Create & configure the sockets
        let (lsock, rsock) = UnixDatagram::pair().unwrap();
        for sock in [&lsock, &rsock] {
            setsockopt(&sock, sockopt::RcvBuf, &2000000).expect("Can't set SO_RCVBUF");
            setsockopt(&sock, sockopt::SndBuf, &2000000).expect("Can't set SO_SNDBUF");
        }

        // Add the local descriptor
        lsock.set_nonblocking(true)
            .expect("Failed to make socket nonblocking");
        lsocks.push(lsock);

        // Add the remote descriptor
        set_cloexec(rsock.as_raw_fd(), false);
        rsocks.push(rsock);
    }

    // Run the upwards command
    let argmap : HashMap<String, String> =
        (0..(lsocks.len()))
        .map(|j| {
            let fd = rsocks[j].as_raw_fd();
            (format!("{{fd{}}}", j), format!("{}", fd))
        }).collect();
    let args_interp : Vec<String> = cmd[1..].iter()
    .map(|s| {
        let mut sr : String = s.clone();
        for (k, v) in argmap.iter() {
            sr = sr.replace(k, v);
        }
        sr
    })
    .collect();
    println!("running command: {} args: {:?}", cmd[0], args_interp);
    let mut cmd0 = Command::new(cmd[0].clone());
    let cmd = cmd0.args(args_interp);
    if let Some(file_name) = stderr {
        let stderr_file = File::create(file_name)
            .expect("Can't create stderr output file");
        cmd.stderr(Stdio::from(stderr_file));
    }
    let mut proc = cmd.spawn().expect("Could not start subprocess");

    // Ignore termination signals
    unsafe {
        sigaction(Signal::SIGINT, &SigAction::new(SigHandler::SigIgn,
            SaFlags::empty(), SigSet::empty()))
            .expect("sigaction failed");
    };

    // Close unused FDs
    drop(rsocks);

    // Start the forwarder 
    println!("Starting the forwarder.");
    forward(fd_outside,
        fd_pipe,
        local_addr,
        remote_addr,
        port_pairs,
        lsocks);

    // Clean up the subprocess
    println!("Killing child");
    proc.kill().expect("kill() failed");
    proc.wait().expect("wait() failed");

    Ok(())
}
