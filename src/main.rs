use core::str;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

const SERVER: Token = Token(0);

struct Server {
    sockets: HashMap<Token, TcpStream>,
    poll: Poll,
}

impl Server {
    fn new() -> std::io::Result<Self> {
        Ok(Server {
            sockets: HashMap::new(),
            poll: Poll::new()?,
        })
    }

    fn handle_new_connections(&mut self, listener: &TcpListener) -> std::io::Result<()> {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let fd = stream.as_raw_fd();
                let client = Token(fd as usize);
                self.poll.registry().register(
                    &mut stream,
                    client,
                    Interest::READABLE | Interest::WRITABLE,
                )?;
                self.sockets.insert(client, stream);
                println!("Got a connection from: {}", fd);
            }
            Err(ref err) if would_block(err) => {}
            Err(err) => {
                eprintln!("Error accepting connection: {}", err);
            }
        }
        Ok(())
    }

    fn handle_user_event(&mut self, client: Token) -> std::io::Result<()> {
        let stream = self.sockets.get_mut(&client).unwrap();
        let mut buf = [0; 256];
        match stream.read(&mut buf) {
            Ok(0) => {
                self.sockets.remove(&client);
                println!("Connection closed fd: {:?}", client.0);
            }
            Ok(n) => {
                println!("Received data: {:?}", &buf[0..n]);
                stream.write_all(&buf)?;
            }
            Err(ref e) if would_block(e) => {}
            Err(e) => {
                eprintln!("Error reading from socket: {}", e);
                self.sockets.remove(&client);
            }
        }
        Ok(())
    }

    fn run(&mut self, addr: &str) -> std::io::Result<()> {
        let mut listener = TcpListener::bind(addr.parse().expect("parse error"))?;
        let mut events = Events::with_capacity(128);
        self.poll
            .registry()
            .register(&mut listener, SERVER, Interest::READABLE)?;

        loop {
            self.poll.poll(&mut events, None)?;
            for event in events.iter() {
                match event.token() {
                    SERVER => self.handle_new_connections(&listener)?,
                    client => self.handle_user_event(client)?,
                }
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut server = Server::new()?;
    server.run("127.0.0.1:10000")?;
    Ok(())
}

fn would_block(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock
}
