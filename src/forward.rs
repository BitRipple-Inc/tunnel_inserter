/*
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
===================================== IMPORTS =====================================
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
*/
/*
>>>>>>>>>>>>>>>>>>>>>>>>>>>>>> EXTERNAL IMPORTS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
*/
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::File;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::os::fd::AsFd;
use std::os::unix::net::UnixDatagram;

/*
>>>>>>>>>>>>>>>>>>>>>>>>>>>>>> INTERNAL IMPORTS >>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
*/
use crate::udp::{create_ipv4_udp_packet, parse_ipv4_udp_packet};

/*
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
==================================== MAIN CODE ====================================
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
*/
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct PortPair {
  pub local: u16,
  pub remote: u16,
}

pub fn forward(
  outside: &UnixDatagram,
  pipe: &File,
  local_addr: Ipv4Addr,
  remote_addr: Ipv4Addr,
  port_pairs: &[PortPair],
  sockets: &[UnixDatagram], // local sockets
) {
  assert_eq!(port_pairs.len(), sockets.len());

  // Create the set of poll file descriptors
  let n = port_pairs.len();
  let mut poll_fds: Vec<PollFd> = sockets
    .iter()
    .map(|d| PollFd::new(d.as_fd(), PollFlags::POLLIN))
    .collect();
  poll_fds.push(PollFd::new(outside.as_fd(), PollFlags::POLLIN));
  poll_fds.push(PollFd::new(pipe.as_fd(), PollFlags::POLLIN));

  // Compute an inverted port pair index
  let pp2idx: HashMap<PortPair, usize> = port_pairs
    .iter()
    .enumerate()
    .map(|(j, pp)| (*pp, j))
    .collect();

  // Poll loop
  let mut buf: Vec<u8> = vec![0u8; 4096];
  'm: loop {
    poll(&mut poll_fds, PollTimeout::NONE).expect("poll failed");
    for (j, pf) in poll_fds.iter().enumerate() {
      let rev = pf.revents().unwrap_or(PollFlags::empty());
      if !rev.intersects(PollFlags::POLLIN | PollFlags::POLLHUP) {
        continue;
      }
      // Check the control pipe
      if j == n + 1 {
        // Termination signal.  Stop.
        println!("Control pipe closed");
        break 'm;
      }
      // Process the other FDs
      //
      // For all of them, we're only listening in this loop.
      if !rev.intersects(PollFlags::POLLIN) {
        continue;
      }
      match j.cmp(&n) {
        Ordering::Less => {
          // j < n: Handle local sockets
          let sz = sockets[j].recv(&mut buf).expect("recv failed");
          //println!("Packet of size {} received from FD {}", sz, j);
          let pkt = create_ipv4_udp_packet(
            &buf[..sz],
            local_addr,
            remote_addr,
            port_pairs[j].local,
            port_pairs[j].remote,
          );
          match outside.send(&pkt) {
            Ok(_) => {}
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
              println!("drop when sending to outside");
            }
            Err(ref e) => {
              eprintln!("Sending to outside failed: {e:?}");
            }
          }
        }
        Ordering::Equal => {
          // j == n: Handle outside socket
          let sz = outside.recv(&mut buf).expect("recv failed");
          //println!("Packet of size {} received from OUTSIDE", sz);
          match parse_ipv4_udp_packet(&buf[..sz]) {
            Some((src_ip, dst_ip, src_port, dst_port, data)) => {
              if src_ip != remote_addr {
                eprintln!("Source IP mismatch.  Expected {remote_addr}, got {src_ip}.",);
                continue;
              }
              if dst_ip != local_addr {
                eprintln!("Destination IP mismatch.  Expected {local_addr}, got {dst_ip}.",);
                continue;
              }
              match pp2idx.get(&PortPair {
                local: dst_port,
                remote: src_port,
              }) {
                None => eprintln!("No matching port pair found"),
                Some(&idx) => match sockets[idx].send(data) {
                  Ok(_) => {}
                  Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    println!("drop when sending to fd{idx}");
                  }
                  Err(ref e) => {
                    eprintln!("error when sending to fd{idx}: {e:?}");
                  }
                },
              }
            }
            None => {
              eprintln!("Invalid packet received on outside");
            }
          }
        }
        Ordering::Greater => {
          // j > n: This case is already handled above (j == n + 1 for control pipe)
          // Since you're already handling j == n + 1 before this section,
          // this branch should theoretically never be reached in your current logic
        }
      }
    }
  }
}
