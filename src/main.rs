use core::str;
use mio::net::{TcpListener, TcpStream};
use mio::{event::Event, Events, Interest, Poll, Token};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

const SERVER: Token = Token(0);

struct User {
    stream: TcpStream,
    name: Option<String>,
    queue: VecDeque<String>,
}

impl User {
    fn new(stream: TcpStream) -> Self {
        User {
            stream,
            name: None,
            queue: VecDeque::new(),
        }
    }

    fn push_message(&mut self, message: String) {
        self.queue.push_back(message)
    }

    fn pop_message(&mut self) -> Option<String> {
        self.queue.pop_front()
    }

    fn write(&mut self) -> std::io::Result<()> {
        while let Some(msg) = self.pop_message() {
            match self.stream.write(msg.as_bytes()) {
                Ok(_) => (),
                Err(ref e) if would_block(e) => break,
                Err(e) => {
                    eprintln!("could not write message to {:?}, {}", self.name, e)
                }
            }
        }
        Ok(())
    }
}

struct Server {
    sockets: HashMap<Token, User>,
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
                let mut user = User::new(stream);
                user.push_message("Please enter your name: ".into());
                self.sockets.insert(client, user);
                println!("Got a connection from: {}", fd);
            }
            Err(ref err) if would_block(err) => {}
            Err(err) => {
                eprintln!("Error accepting connection: {}", err);
            }
        }
        Ok(())
    }

    fn handle_user_event(&mut self, client: Token, event: &Event) -> std::io::Result<()> {
        if event.is_read_closed() {
            self.sockets.remove(&client);
            println!("Connection closed fd: {:?}", client.0);
            return Ok(());
        }

        if event.is_readable() {
            if let Some(user) = self.sockets.get_mut(&client) {
                let mut buf = [0; 256];
                match user.stream.read(&mut buf) {
                    Ok(n) => {
                        if user.name.is_none() {
                            if let Ok(name) = str::from_utf8(&buf[..n]) {
                                user.name = Some(name.trim().to_string());
                                println!("User connected with name: {}", name.trim());
                                user.push_message(
                                    "Welcome, you can start sending messages.\n".into(),
                                );
                                self.poll.registry().reregister(
                                    &mut user.stream,
                                    client,
                                    Interest::READABLE | Interest::WRITABLE,
                                )?;
                            }
                        } else {
                            println!("Received data: {:?}", &buf[0..n]);
                            let message = format!(
                                "{}: {}",
                                user.name.clone().unwrap_or_default(),
                                String::from_utf8_lossy(&buf[..n])
                            );
                            for (token, other_user) in self.sockets.iter_mut() {
                                if *token != client {
                                    other_user.push_message(message.clone());
                                    self.poll.registry().reregister(
                                        &mut other_user.stream,
                                        *token,
                                        Interest::READABLE | Interest::WRITABLE,
                                    )?;
                                }
                            }
                        }
                    }
                    Err(ref e) if would_block(e) => {}
                    Err(e) => {
                        eprintln!("Error reading from socket: {}", e);
                        self.sockets.remove(&client);
                    }
                }
            }
        }

        if event.is_writable() {
            if let Some(user) = self.sockets.get_mut(&client) {
                user.write()?;
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
                    client => self.handle_user_event(client, event)?,
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
