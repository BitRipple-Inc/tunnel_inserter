# What's this?

Adapter tool that receives raw IP datagrams on an outside socket,
unwraps the UDP payload and sends them to other sockets given on the
command line.  Conversely, data received from those other sockets is
wrapped with a UDP header, and sent out the outside socket.

Example:
```sh
tunnel_inserter \
    --outside 10 --control 11 \
    --local-addr 192.168.12.1
    --remote-addr 192.168.12.2
    --local-ports 2000 2001 \
    --remote-ports 3000 3001 \
    -- \
    my_command --foo '{fd0}' --bar '{fd1}'
```
Causes tunnel inserter to start the executable `my_command` with the
provided arguments above.  The `{fd0}` and `{fd1}` place holders are
replaced by integer unix datagram socket file descriptors opened by
`tunnel_inserter`.

Any datagram received by `tunnel_inserter` on FD 10 is verified to be a
raw UDP packet with source address 192.168.12.2 and destination address
192.168.12.1.  If it is, then
- If the source port is 3000 and the destination port is 2000, the
  packet payload is sent as a datagram to `my_command`'s fd0.
- If the source port is 3001 and the destination port is 2001, the
  packet payload is sent as a datagram to `my_command`'s fd1.
- Otherwise, the packet is dropped.

Likewise, any datagram received from `my_command`'s fd0 gets a UDP
header prepended with source address 192.168.12.1, destination address
192.168.12.2, source port 2000 and destination port 3000, and is sent
out on FD 10.  Similarly, datagrams received on fd1 are treated the same
except that the port numbers are (2001, 3001) in this case.

# Notes

- Interfaces to lightway:
  - `--outside`: to/from the outside
  - `--control`: the read end of a control pipe.  The tool shuts down
    when the write end is closed.
  Note:  There is no `--inside`:  This input is currently directly wired
  through from lightway to the tunnel inherited through
  `tunnel_inserter`; tunnel inserter does not touch it.

- Interfaces to the bitripple tunnel:
  - feedback send
  - feedback receive
  - data send
  - data receive
