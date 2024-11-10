use core::str;
use mio::net::{TcpListener, TcpStream};
use mio::{event::Event, Events, Interest, Poll, Token};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::rc::Rc;

const SERVER: Token = Token(0);

#[derive(Debug)]
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

    fn read(&mut self) -> std::io::Result<Option<String>> {
        let mut buf = [0; 256];
        let data = match self.stream.read(&mut buf) {
            Ok(n) => std::str::from_utf8(&buf[0..n]).expect("ignoring error"),
            Err(ref e) if would_block(e) => "",
            Err(e) => {
                eprintln!("Error reading from socket: {}", e);
                return Err(e);
            }
        };

        if self.name.is_none() {
            self.name = Some(data.to_string());
            println!("User connected with name: {:?}", self.name);
            Ok(None)
        } else {
            println!("Received data: {:?}", data);
            Ok(Some(data.to_string()))
        }
    }
}

struct Server {
    sockets: Rc<RefCell<HashMap<Token, Rc<RefCell<User>>>>>,
    poll: Poll,
}

impl Server {
    fn new() -> std::io::Result<Self> {
        Ok(Server {
            sockets: Rc::new(RefCell::new(HashMap::new())),
            poll: Poll::new()?,
        })
    }

    fn broadcast(&mut self, msg: String, cur_token: Token) -> std::io::Result<()> {
        for (token, other_user) in self.sockets.clone().borrow().iter() {
            if *token != cur_token {
                other_user.borrow_mut().push_message(msg.clone());
                self.reregister_rw(other_user.clone(), *token)?;
            }
        }

        Ok(())
    }

    fn reregister_rw(&mut self, user: Rc<RefCell<User>>, token: Token) -> std::io::Result<()> {
        println!("{:?}", user.clone().borrow());
        self.poll.registry().reregister(
            &mut user.borrow_mut().stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        Ok(())
    }

    fn register_rw(&mut self, user: Rc<RefCell<User>>, token: Token) -> std::io::Result<()> {
        println!("{:?}", user.clone().borrow());
        self.poll.registry().register(
            &mut user.borrow_mut().stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        Ok(())
    }

    fn handle_new_connections(&mut self, listener: &TcpListener) -> std::io::Result<()> {
        match listener.accept() {
            Ok((stream, _)) => {
                let fd = stream.as_raw_fd();
                let token = Token(fd as usize);

                let user = Rc::new(RefCell::new(User::new(stream)));
                self.register_rw(user.clone(), token)?;
                user.borrow_mut()
                    .push_message("Please enter your name: ".into());
                self.sockets.clone().borrow_mut().insert(token, user);

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
            self.sockets.clone().borrow_mut().remove(&client);
            println!("Connection closed fd: {:?}", client.0);
            return Ok(());
        }

        if event.is_readable() {
            if let Some(user) = self.sockets.clone().borrow().get(&client) {
                let msg = user.borrow_mut().read()?;

                if msg.is_none() {
                    user.borrow_mut()
                        .push_message("Welcome, you can start sending messages.\n".into());
                    self.reregister_rw(user.clone(), client)?;
                }

                if let Some(message) = msg {
                    println!("Received data: {:?}", message);
                    let message = format!(
                        "{}: {}",
                        user.borrow().name.clone().unwrap_or_default(),
                        message
                    );
                    self.broadcast(message, client)?;
                }
            }
        }

        if event.is_writable() {
            if let Some(user) = self.sockets.clone().borrow_mut().get_mut(&client) {
                user.borrow_mut().write()?;
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
