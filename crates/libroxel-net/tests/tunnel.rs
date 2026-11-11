//! Bytes go both ways through a host tunnel and a join tunnel on this
//! computer, with no relay or lookup servers.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use libroxel_net::{HostTunnel, Identity, JoinTunnel, Network};

#[test]
fn a_joiner_reaches_the_host_game() {
    // Stands in for the hosted game: echoes one line back in capitals.
    let game = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = game.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = game.accept().unwrap();
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf).unwrap();
        stream.write_all(&buf[..n].to_ascii_uppercase()).unwrap();
    });

    let host = HostTunnel::start(&Identity::generate(), Network::LocalOnly, port).unwrap();
    let start = Instant::now();
    let code = loop {
        if let Some(code) = host.code() {
            break code;
        }
        assert!(host.error().is_none(), "{:?}", host.error());
        assert!(start.elapsed() < Duration::from_secs(10), "no join code");
        std::thread::sleep(Duration::from_millis(20));
    };

    let join = JoinTunnel::start(&Identity::generate(), Network::LocalOnly, &code).unwrap();
    let mut player = TcpStream::connect(join.local_addr()).unwrap();
    player.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    player.write_all(b"hello host\n").unwrap();
    let mut reply = [0u8; 64];
    let n = player.read(&mut reply).unwrap();
    assert_eq!(&reply[..n], b"HELLO HOST\n", "{:?}", join.error());
    assert!(join.connected());
    server.join().unwrap();
}
