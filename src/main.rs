use core::str;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::error::Error;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

const SERVER: Token = Token(0);

fn main() -> Result<(), Box<dyn Error>> {
    let mut sockets = HashMap::new();

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    let address = "127.0.0.1:10000".parse()?;
    let mut listener = TcpListener::bind(address)?;

    poll.registry()
        .register(&mut listener, SERVER, Interest::READABLE)?;

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let fd = stream.as_raw_fd();
                            let client = Token(fd as usize);
                            poll.registry().register(
                                &mut stream,
                                client,
                                Interest::READABLE | Interest::WRITABLE,
                            )?;
                            sockets.insert(client, stream);
                            println!("Got a connection from: {}, fd: {}", address, fd);
                        }
                        Err(ref err) if would_block(err) => {
                            break;
                        }
                        Err(err) => {
                            eprintln!("Error accepting connection: {}", err);
                            break;
                        }
                    }
                },
                client => {
                    let stream = sockets.get_mut(&client).unwrap();
                    let mut buf = [0; 256];
                    match stream.read(&mut buf) {
                        Ok(0) => {
                            sockets.remove(&client);
                            println!("Connection closed fd: {:?}", client.0);
                        }
                        Ok(n) => {
                            println!("Received data: {:?}", str::from_utf8(&buf[..n])?);
                            stream.write_all(&buf[..n])?;
                        }
                        Err(ref e) if would_block(e) => {
                            break;
                        }
                        Err(e) => {
                            eprintln!("Error reading from socket: {}", e);
                            sockets.remove(&client);
                        }
                    }
                }
            }
        }
    }
}

fn would_block(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock
}
