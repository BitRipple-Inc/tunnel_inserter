#![allow(dead_code)]

use std::net::Ipv4Addr;

const IPV4_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

/// Compute one's complement checksum for a given buffer
pub fn checksum(mut data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    while data.len() >= 2 {
        sum += u16::from_be_bytes([data[0], data[1]]) as u32;
        data = &data[2..];
    }

    if !data.is_empty() {
        sum += (data[0] as u32) << 8;
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}

/// Creates a valid IPv4 UDP packet
pub fn create_ipv4_udp_packet(
    payload: &[u8], 
    src_ip: Ipv4Addr,//[u8; 4], 
    dst_ip: Ipv4Addr,//[u8; 4], 
    src_port: u16, 
    dst_port: u16
) -> Vec<u8> {
    let udp_length = UDP_HEADER_LEN + payload.len();
    let total_length = IPV4_HEADER_LEN + udp_length;

    let mut packet = vec![0u8; total_length];

    // IPv4 Header
    packet[0] = 0x45; // Version (4) + IHL (5)
    packet[1] = 0x00; // DSCP + ECN
    packet[2..4].copy_from_slice(&(total_length as u16).to_be_bytes()); // Total length
    packet[4..6].copy_from_slice(&0x0000u16.to_be_bytes()); // Identification
    packet[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // Flags + Fragment offset
    packet[8] = 64; // TTL
    packet[9] = 17; // Protocol (UDP)
    packet[12..16].copy_from_slice(&src_ip.octets()); // Source IP
    packet[16..20].copy_from_slice(&dst_ip.octets()); // Destination IP

    // Compute IPv4 Header Checksum
    let ip_checksum = checksum(&packet[..IPV4_HEADER_LEN]);
    packet[10..12].copy_from_slice(&ip_checksum.to_be_bytes());

    // UDP Header
    let udp_offset = IPV4_HEADER_LEN;
    packet[udp_offset..udp_offset + 2].copy_from_slice(&src_port.to_be_bytes());
    packet[udp_offset + 2..udp_offset + 4].copy_from_slice(&dst_port.to_be_bytes());
    packet[udp_offset + 4..udp_offset + 6].copy_from_slice(&(udp_length as u16).to_be_bytes());

    // Copy Payload
    let payload_offset = udp_offset + UDP_HEADER_LEN;
    packet[payload_offset..].copy_from_slice(payload);

    // Compute UDP Checksum (with pseudo-header)
    if false {
        let mut pseudo_header = Vec::new();
        pseudo_header.extend_from_slice(&src_ip.octets());
        pseudo_header.extend_from_slice(&dst_ip.octets());
        pseudo_header.push(0); // Zero byte
        pseudo_header.push(17); // Protocol (UDP)
        pseudo_header.extend_from_slice(&(udp_length as u16).to_be_bytes());
        pseudo_header.extend_from_slice(&packet[udp_offset..udp_offset + UDP_HEADER_LEN + payload.len()]);

        let udp_checksum = checksum(&pseudo_header);
        packet[udp_offset + 6..udp_offset + 8].copy_from_slice(&udp_checksum.to_be_bytes());
    } else {
        packet[udp_offset + 6..udp_offset + 8].copy_from_slice(&[0, 0]);
    }

    packet
}

/// Parses a raw IPv4 UDP packet and extracts relevant information
pub fn parse_ipv4_udp_packet(packet: &[u8]) -> Option<(Ipv4Addr, Ipv4Addr, u16, u16, &[u8])> {
    if packet.len() < IPV4_HEADER_LEN + UDP_HEADER_LEN {
        println!("Packet too short to be a valid IPv4 UDP packet.");
        return None;
    }

    // Extract IPv4 Header Fields
    let ihl = (packet[0] & 0x0F) as usize * 4;
    if ihl < IPV4_HEADER_LEN {
        println!("Invalid IPv4 header length: {}", ihl);
        return None;
    }

    let total_length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if total_length != packet.len() {
        println!("Packet length mismatch: Expected {}, Found {}", total_length, packet.len());
        return None;
    }

    let protocol = packet[9];
    if protocol != 17 {
        println!("Not a UDP packet (protocol = {}).", protocol);
        return None;
    }

    let src_ip = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let dst_ip = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);

    // Verify IPv4 Header Checksum
    let ip_checksum = checksum(&packet[..ihl]);
    if ip_checksum != 0 {
        println!("Invalid IPv4 header checksum: {}", ip_checksum);
        return None;
    }

    // Extract UDP Header Fields
    let udp_offset = ihl;
    let src_port = u16::from_be_bytes([packet[udp_offset], packet[udp_offset + 1]]);
    let dst_port = u16::from_be_bytes([packet[udp_offset + 2], packet[udp_offset + 3]]);
    let udp_length = u16::from_be_bytes([packet[udp_offset + 4], packet[udp_offset + 5]]) as usize;

    if udp_length < UDP_HEADER_LEN || udp_offset + udp_length > packet.len() {
        println!(
            "UDP length mismatch: Expected {}, Packet size {}", 
            udp_length, 
            packet.len()
        );
        return None;
    }

    let udp_checksum = u16::from_be_bytes([packet[udp_offset + 6], packet[udp_offset + 7]]);
    let payload = &packet[udp_offset + UDP_HEADER_LEN..udp_offset + udp_length];

    // Compute UDP checksum (including pseudo-header)
    if udp_checksum != 0 {
        let mut pseudo_header = Vec::new();
        pseudo_header.extend_from_slice(&src_ip.octets());
        pseudo_header.extend_from_slice(&dst_ip.octets());
        pseudo_header.push(0);
        pseudo_header.push(17); // Protocol (UDP)
        pseudo_header.extend_from_slice(&(udp_length as u16).to_be_bytes());
        pseudo_header.extend_from_slice(&packet[udp_offset..udp_offset + udp_length]);

        let computed_udp_checksum = checksum(&pseudo_header);
        if udp_checksum != 0 && computed_udp_checksum != 0 {
            println!(
                "Invalid UDP checksum: Expected {}, Computed {}", 
                udp_checksum, 
                computed_udp_checksum
            );
            return None;
        }
    }

    Some((src_ip, dst_ip, src_port, dst_port, payload))
}

// Run a couple of test cases.

#[cfg(test)]
mod tests {

    use crate::Ipv4Addr;
    use crate::udp;

    fn analyze_pkt(pkt: &[u8]) {
        match udp::parse_ipv4_udp_packet(pkt) {
            Some((src_ip, dst_ip, src_port, dst_port, payload)) => {
                println!("Valid IPv4 UDP Packet:");
                println!("  Source IP: {}", src_ip);
                println!("  Destination IP: {}", dst_ip);
                println!("  Source Port: {}", src_port);
                println!("  Destination Port: {}", dst_port);
                println!("  Payload: {:?}", String::from_utf8_lossy(payload));
            }
            None => {
                println!("Invalid packet.");
                panic!();
            }
        }
    }

    #[test]
    fn example_raw_decode() {
        // Example raw UDP packet (in hex)
        let raw_packet: Vec<u8> = vec![
            0x45, 0x00, 0x00, 0x22, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0xB7, 0x15, // IPv4 Header (example)
            192, 168, 1, 100, // Source IP
            192, 168, 1, 1,   // Destination IP
            0x30, 0x39, 0x00, 0x50, 0x00, 0x0E, 0x00, 0x00, // UDP Header (example)
            72, 101, 108, 108, 111, 33 // "Hello!"
        ];

        analyze_pkt(&raw_packet);
    }

    #[test]
    fn example_encapsulate_decapsulate() {

        // other way.
        let payload = b"Hello, UDP!";
        let src_ip = Ipv4Addr::new(192, 168, 1, 100);
        let dst_ip = Ipv4Addr::new(192, 168, 1, 1);
        let src_port = 12345;
        let dst_port = 80;

        let packet = udp::create_ipv4_udp_packet(payload, src_ip, dst_ip, src_port, dst_port);
        println!("Generated IPv4 UDP Packet: {:02X?}", packet);

        println!("\n\nNow analyzing this packet.");
        analyze_pkt(&packet);
    }
}
