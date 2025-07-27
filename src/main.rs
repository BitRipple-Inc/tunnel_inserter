use clap::{arg, value_parser};
use std::net::Ipv4Addr;

use tunnel_inserter::{TunnelInserter, TunnelInserterConfig};

fn main() -> Result<(), String> {
    let matches = clap::Command::new("tunnel_inserter")
        .about("Forwards raw packets and starts the BitRipple/Axl tunnel")
        .arg(arg!(-o --outside <OUTSIDE_FD> "Socket corresponding to outside").value_parser(value_parser!(i32)).required(true))
        .arg(arg!(-c --control <CONTROL_FD> "Control pipe file descriptor").value_parser(value_parser!(i32)).required(true))
        .arg(arg!(--"local-addr" <IP> "Local IPv4 address").value_parser(value_parser!(Ipv4Addr)).required(true))
        .arg(arg!(--"remote-addr" <IP> "Remote IPv4 address").value_parser(value_parser!(Ipv4Addr)).required(true))
        .arg(arg!(--"local-ports" <PORTS> "Local ports (space separated)").value_parser(value_parser!(u16)).num_args(1..).required(false))
        .arg(arg!(--"remote-ports" <PORTS> "Remote ports (space separated)").value_parser(value_parser!(u16)).num_args(1..).required(false))
        .arg(arg!(--"stderr-file" <FILE> "Destination for stderr").required(false))
        .arg(arg!(<CMD> "Command to call").num_args(1..).required(true))
        .get_matches();

    let cfg = TunnelInserterConfig {
        outside_fd: *matches.get_one::<i32>("outside").unwrap(),
        control_fd: *matches.get_one::<i32>("control").unwrap(),
        local_addr: *matches.get_one::<Ipv4Addr>("local-addr").unwrap(),
        remote_addr: *matches.get_one::<Ipv4Addr>("remote-addr").unwrap(),
        local_ports: matches.get_many::<u16>("local-ports").map(|p| p.copied().collect()).unwrap_or_default(),
        remote_ports: matches.get_many::<u16>("remote-ports").map(|p| p.copied().collect()).unwrap_or_default(),
        stderr_file: matches.get_one::<String>("stderr-file").cloned(),
        axlrust_args: matches.get_many::<String>("CMD").unwrap().map(|s| s.to_string()).collect(),
    };

    TunnelInserter::new(cfg).run()
}
