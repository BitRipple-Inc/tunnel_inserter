/*
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
===================================== IMPORTS =====================================
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
*/
/*
>>>>>>>>>>>>>>>>>>>>>>>>>>>>>> EXTERNAL IMPORTS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
*/
use clap::{arg, value_parser};
use std::fs::File;
use std::collections::{HashMap};
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::{UnixDatagram};
use std::process::{Command, Stdio};

use nix::sys::signal::{sigaction, SaFlags, SigSet, Signal, SigAction, SigHandler};
use nix::sys::socket::{setsockopt, sockopt};
/*
>>>>>>>>>>>>>>>>>>>>>>>>>>>>>> INTERNAL IMPORTS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
*/
use crate::sock_utils::set_cloexec;
use crate::forward::{PortPair, forward};

mod udp;
mod sock_utils;
mod forward;

/*
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
===================================== MAIN CODE ===================================
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
*/
fn main() -> Result<(), String> {
    /*
     * ============================================================================
     * >>>>>>>>>>>>>>>>>>>>>>>>>> COMMAND LINE ARGUMENTS >>>>>>>>>>>>>>>>>>>>>>>>>>
     * ============================================================================
     * 1. `outside`: File descriptor for the lightway-created socket,
     *               where UDP packets are sent and received.
     * 2. `control`: File descriptor of a control pipe for signaling/coordination
     * 3. `local-addr`: IP address corresponding to 
     * 4. `remote-addr`: IP address corresponding to 
     * 5. `local-ports`: Space-separated list of ports where the target program expects to receive raw data payloads.
     * 6. `remote-ports`: Analogous list of ports as `local-ports` from which the remote program is sending the UDP packets.
     */
    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> MATCHING ARGUMENTS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
    let matches = clap::Command::new("tunnel_inserter")
        .about("Forwards raw packets to <-> from lightway into \
            a tool such as bitripple tunnel.")
        // ----------------- OUTSIDE-CONNECTED FILE DESCRIPTORS -----------------
        .arg(arg!(-o --outside <OUTSIDE_FD> "Socket corresponding to outside")
            .value_parser(value_parser!(i32))
            .required(true))
        .arg(arg!(-c --control <CONTROL_FD> "Control pipe file descriptor")
            .value_parser(value_parser!(i32))
            .required(true))
        // ---------------------------- IP ADDRESSES ----------------------------
        .arg(arg!(--"local-addr" <IP> "Local IPv4 address")
            .value_parser(value_parser!(Ipv4Addr))
            .required(true))
        .arg(arg!(--"remote-addr" <IP> "Remote IPv4 address")
            .value_parser(value_parser!(Ipv4Addr))
            .required(true))
        // -------------------------------- PORTS ---------------------------------
		.arg(arg!(--"local-ports" <PORTS> "Local ports (space separated)")
            .value_parser(value_parser!(u16))
            .num_args(1..)
            .required(false))
        .arg(arg!(--"remote-ports" <PORTS> "Remote ports (space separated)")
            .value_parser(value_parser!(u16))
            .num_args(1..)
            .required(false))
        // ------------------------------ ERROR LOG --------------------------------
        .arg(arg!(--"stderr-file" <FILE> "Destination for stderr")
            .required(false))
        // ----------------- PROGRAM THAT PROCESSES RAW DATA PACKETS -----------------
        .arg(arg!(<CMD> "Command to call")
            .num_args(1..)
            .required(true))
        .get_matches();

    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> HANDLING ARGUMENTS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
    // -------------------- OUTSIDE-CONNECTED FILE DESCRIPTORS -------------------
    let fd_outside_i = *matches.get_one::<i32>("outside").unwrap();
    let fd_pipe_i = *matches.get_one::<i32>("control").unwrap();
    // ----------------- PROGRAM THAT PROCESSES RAW DATA PACKETS -----------------
    let cmd: Vec<String> = matches.get_many::<String>("CMD")
        .unwrap()
        .map(|s| { s.to_string() })
        .collect();
    // ---------------------------- IP ADDRESSES ----------------------------
	let local_addr = matches.get_one::<Ipv4Addr>("local-addr");
    let local_addr : Ipv4Addr = match local_addr {
        Some(&x) => x,
        None => return Err(String::from("local address not set")),
    };
    let remote_addr = matches.get_one::<Ipv4Addr>("remote-addr");
    let remote_addr : Ipv4Addr = match remote_addr {
        Some(&x) => x,
        None => return Err(String::from("remote address not set")),
    };
    // -------------------------------- PORTS ---------------------------------
    // There might be multiple ports (space-separated in arguments) corresponding
    // to the number of file descriptors the program CMD is expecting as arguments.
    // e.g. `CMD --foo '{fd0}' --bar '{fd1}'` would require 2 pairs of (local, remote) ports.
	let local_ports: Vec<u16> = matches
        .get_many::<u16>("local-ports")
        .map(|ports| ports.cloned().collect())
        .unwrap_or_default();
    let remote_ports: Vec<u16> = matches
        .get_many::<u16>("remote-ports")
        .map(|ports| ports.cloned().collect())
        .unwrap_or_default();
    if local_ports.len() != remote_ports.len() {
            return Err("Need the same number of --local-port as \
            --remote-port".to_string());
    }
    // ------------------------------ ERROR LOG --------------------------------
    let stderr : Option<&String> = matches.get_one::<String>("stderr-file");

    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> SOME DEBUGGING INFO >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
    println!("Settings:");
    println!("fd_outside {}, fd_pipe {}",
        fd_outside_i,
        fd_pipe_i);
    println!("cmd: {:?}", cmd);
    println!("local_addr: {:?}", local_addr);
    println!("remote_addr: {:?}", remote_addr);
    println!("local_ports: {:?}", local_ports);
    println!("remote_ports: {:?}", remote_ports);


    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> CREATING DOWNWARD SOCKETS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
    // These come from lightway
    let fd_outside = unsafe { UnixDatagram::from_raw_fd(fd_outside_i) };
    let fd_pipe = File::from(unsafe { OwnedFd::from_raw_fd(fd_pipe_i) });
    // Ensures FDs are not inherited by another program when exec-functions are called
    set_cloexec(fd_outside_i, true);
    set_cloexec(fd_pipe_i, true);
    // We want to be non-blocking
    fd_outside.set_nonblocking(true)
        .expect("Failed to make socket nonblocking");

    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> CREATING UPWARD SOCKETS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
    let mut port_pairs : Vec<PortPair> = vec![]; 
    let mut lsocks : Vec<UnixDatagram> = vec![]; // ===== local sockets
    let mut rsocks : Vec<UnixDatagram> = vec![]; // ===== remote sockets
    /*
        For each pair of sockets:
        1. Add them to `port_pairs` used by the forwarder.
        2. Create & configure the inter-process UDP sockets with specified buffer size.
        3. Add the local descriptor to `lsocks`, which is used by the forwarder, after making it non-blocking
        4. Add the remote descriptor to `rsocks`, which is used by the forwarder, after making it non-inheritable after exec-functions
     */
    for (l, r) in local_ports.into_iter().zip(remote_ports) {
        // 1. Add the port pair
        port_pairs.push(PortPair { local: l, remote: r });
        // 2. Create & configure the inter-process UDP sockets with specified buffer size.
        let (lsock, rsock) = UnixDatagram::pair().unwrap();
        for sock in [&lsock, &rsock] { // Set Maximum receive/send buffer size to 2 megabytes
            setsockopt(&sock, sockopt::RcvBuf, &2000000).expect("Can't set SO_RCVBUF");
            setsockopt(&sock, sockopt::SndBuf, &2000000).expect("Can't set SO_SNDBUF");
        }
        // 3. Add the local descriptor after making it non-blocking
        lsock.set_nonblocking(true)
            .expect("Failed to make socket nonblocking");
        lsocks.push(lsock);
        // 4. Add the remote descriptor
        set_cloexec(rsock.as_raw_fd(), false);
        rsocks.push(rsock);
    }

    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> RUNNING UPWARD COMMAND >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
    // <Local_socket_index, Remote_socket>, to 
    let argmap : HashMap<String, String> =
        (0..(lsocks.len()))
        .map(|j| {
            let fd = rsocks[j].as_raw_fd();
            (format!("{{fd{}}}", j), format!("{}", fd)) // Replaces the j-th fd (fdj) with j-th remote socket. 
        }).collect();
    // Since the only value for each argument is the FD, we substitute the
    let args_interp: Vec<String> = cmd[1..].iter()
    .map(|s| {
        let mut sr : String = s.clone();
        for (k, v) in argmap.iter() {
            sr = sr.replace(k, v);
        }
        sr
    })
    .collect();
    // Run the command CMD with the remote file descriptors as new arguments
    println!("running command: {} args: {:?}", cmd[0], args_interp);
    let mut cmd0 = Command::new(cmd[0].clone());
    let cmd = cmd0.args(args_interp);
    if let Some(file_name) = stderr {
        let stderr_file = File::create(file_name)
            .expect("Can't create stderr output file");
        cmd.stderr(Stdio::from(stderr_file));
    }
    let mut proc = cmd.spawn().expect("Could not start subprocess");

    /*
    >>>>>>>>>>>>>>>>>>>>>>>>>>>>>> HANDLE CLOSING AND FORWARDING >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
    */
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
