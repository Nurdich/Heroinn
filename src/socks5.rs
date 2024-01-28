#![allow(dead_code)]

use std::borrow::Borrow;
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use tokio::io::{
    self, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf,
};
use tokio::net::{TcpListener, TcpStream};

enum AddressType {
    IPv4 = 0x01,
    DomainName = 0x03,
    IPv6 = 0x04,
}

#[derive(PartialEq)]
pub enum AuthMethods {
    NoAuth = 0,
    GsSAPI = 1,
    UsernamePassword = 2,
    IANAAssigned = 3,
    Reserved = 4,
    NotAcceptable = 0xff,
}

impl AuthMethods {
    fn from(value: u8) -> AuthMethods {
        match value {
            0 => AuthMethods::NoAuth,
            1 => AuthMethods::GsSAPI,
            2 => AuthMethods::UsernamePassword,
            3 => AuthMethods::IANAAssigned,
            4 => AuthMethods::Reserved,
            0xff => AuthMethods::NotAcceptable,
            _ => panic!("Invalid value"),
        }
    }

    fn clone(&self) -> AuthMethods {
        match self {
            AuthMethods::NoAuth => AuthMethods::NoAuth,
            AuthMethods::GsSAPI => AuthMethods::GsSAPI,
            AuthMethods::UsernamePassword => {
                AuthMethods::UsernamePassword
            }
            AuthMethods::IANAAssigned => AuthMethods::IANAAssigned,
            AuthMethods::Reserved => AuthMethods::Reserved,
            AuthMethods::NotAcceptable => AuthMethods::NotAcceptable,
        }
    }

    fn to_u8(&self) -> u8 {
        match self {
            AuthMethods::NoAuth => 0,
            AuthMethods::GsSAPI => 1,
            AuthMethods::UsernamePassword => 2,
            AuthMethods::IANAAssigned => 3,
            AuthMethods::Reserved => 4,
            AuthMethods::NotAcceptable => 0xff,
        }
    }
}

pub struct Proxy {
    auth_methods: Vec<AuthMethods>,
    users: Mutex<HashMap<String, String>>,
}

impl Proxy {
    pub fn new(auth_methods: Vec<AuthMethods>) -> Proxy {
        Proxy {
            auth_methods,
            users: Mutex::new(HashMap::new()),
        }
    }

    // User operations
    pub fn add_user(&mut self, username: String, password: String) {
        self.users.lock().unwrap().insert(username, password);
    }

    pub fn remove_user(&mut self, username: String) {
        self.users.lock().unwrap().remove(&username);
    }

    pub fn get_user(&self, username: String) -> Option<String> {
        self.users.lock().unwrap().get(&username).cloned()
    }

    pub fn check_valid_auth_method(
        &self,
        auth_method: &AuthMethods,
    ) -> bool {
        return self.auth_methods.contains(auth_method);
    }
}

async fn bidirectional_streaming(
    mut reader: ReadHalf<TcpStream>,
    mut writer: WriteHalf<TcpStream>,
) {
    let mut buf = [0; 1024];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                if writer.write_all(&buf[..n]).await.is_err() {
                    break; // Error or connection closed
                }
            }
            Err(_) => break, // Error reading
        }
    }
}

async fn read_address(
    buf: &[u8],
) -> Result<(String,), Box<dyn Error>> {
    Ok((String::from(""),))
}

pub fn check_valid_version(version: &u8) -> bool {
    // Check if version is 5
    return 0x05 == *version;
}

pub async fn start_proxy(
    proxy: Arc<Proxy>,
    server_addr: Option<String>,
    server_port: Option<i32>,
) -> Result<(), Box<dyn Error>> {
    let proxy_addr = String::from(format!(
        "{}:{}",
        server_addr.unwrap_or(String::from("0.0.0.0")),
        server_port.unwrap_or(1080)
    ));

    let listener = TcpListener::bind(proxy_addr.clone()).await?;

    println!("Proxys listening on: {}", proxy_addr);

    loop {
        let (socket, _) = listener.accept().await?;
        let copy = proxy.clone();
        tokio::spawn(async move {
            handle_connection(copy, socket).await;
        });
    }
}

async fn handle_connection(proxy: Arc<Proxy>, mut socket: TcpStream) {
    let mut buf: [u8; 258] = [0; 258];

    match socket.read(&mut buf).await {
        Ok(n) => {
            if !check_valid_version(&buf[0]) {
                println!("Not socks5 version");
                return;
            }

            // Auth Checking
            let nmethod: u8 = buf[1];

            let methods = &buf[2..(2 + nmethod as usize)];

            // Server has two auth methods
            // UsernamePassword and NoAuth
            // usernamePassword  is most priority
            // check first if usernamePassword is available
            if (methods
                .contains(&AuthMethods::UsernamePassword.to_u8())
                && proxy.check_valid_auth_method(
                    &AuthMethods::UsernamePassword,
                ))
            {
                println!("UsernamePassword");

                // Write
                match socket
                    .write(&[
                        5,                                     // Version
                        AuthMethods::UsernamePassword.to_u8(), // UsernamePassword
                    ])
                    .await
                {
                    Ok(n) => {}
                    Err(e) => {
                        println!("Error sending response");
                        return;
                    }
                }

                // Read UsernamePassword
                let mut buf: [u8; 258] = [0; 258];

                match socket.read(&mut buf).await {
                    Ok(n) => {
                        println!("UsernamePassword: {:?}", buf);

                        let username_length = buf[1];
                        let password_length =
                            buf[2 + username_length as usize];

                        let username =
                            &buf[2..(2 + username_length as usize)];
                        let password = &buf[(3 + username_length
                            as usize)
                            ..(3 + username_length as usize
                                + password_length as usize)];

                        let username =
                            String::from_utf8(username.to_vec())
                                .unwrap();
                        let password =
                            String::from_utf8(password.to_vec())
                                .unwrap();

                        println!("Username: {:?}", username);
                        println!("Password: {:?}", password);

                        match proxy.get_user(username.clone()) {
                            Some(_password) => {
                                println!("User is valid");

                                if _password == password {
                                    // Write
                                    match socket
                                        .write(&[
                                            1, // Version
                                            0, // Succeeded
                                        ])
                                        .await
                                    {
                                        Ok(n) => {
                                            println!(
                                                "ACK Response sent"
                                            );
                                        }
                                        Err(e) => {
                                            println!("Error sending response");
                                        }
                                    }

                                    make_proxy(socket).await;
                                } else {
                                    match socket
                                        .write(&[
                                            1, // Version
                                            1, // Failed
                                        ])
                                        .await
                                    {
                                        Ok(n) => {
                                            println!(
                                                "ACK Response sent"
                                            );
                                        }
                                        Err(e) => {
                                            println!(
                                            "Error sending response"
                                        );
                                        }
                                    }
                                    return;
                                }
                            }
                            None => {
                                println!("User is not valid");

                                // Write
                                match socket
                                    .write(&[
                                        1, // Version
                                        1, // Failed
                                    ])
                                    .await
                                {
                                    Ok(n) => {
                                        println!("ACK Response sent");
                                    }
                                    Err(e) => {
                                        println!(
                                            "Error sending response"
                                        );
                                    }
                                }
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        println!("Error reading from socket: {}", e);
                    }
                }
            } else if (methods
                .contains(&&AuthMethods::NoAuth.to_u8()))
            {
                println!("NoAuth");
            } else {
                println!("NotAcceptable");
            }
        }
        Err(e) => {
            println!("Error reading from socket: {}", e);
        }
    }
}



/**
 *
 * Function to proxy
 *
 *
*/
async fn make_proxy(mut socket: TcpStream) {
    let mut request: [u8; 4] = [0; 4];

    // Read First 4 bytes
    match socket.read(&mut request).await {
        Ok(n) => {
            println!("Request : {:?}", request);

            if !check_valid_version(&request[0]) {
                println!("Not socks5 version");
                return;
            }

            let method = request[1];
            let address_type = request[3];

            if address_type == 1 {
                println!("IPv4");

                // Read 4 bytes For address and 2 bytes for port
                let mut address: [u8; 6] = [0; 6];

                match socket.read(&mut address).await {
                    Ok(n) => {
                        println!("Address: {:?}", address);

                        let port = u16::from_be_bytes([
                            address[4], address[5],
                        ]);

                        println!("Port: {}", port);

                        // Connect to the server
                        let mut server_socket =
                            TcpStream::connect(format!(
                                "{}.{}.{}.{}:{}",
                                address[0],
                                address[1],
                                address[2],
                                address[3],
                                port
                            ))
                            .await
                            .unwrap();

                        // Write response
                        match socket
                            .write(&[
                                5, // Version
                                0, // Succeeded
                                0, // Reserved
                                1, // IPv4
                                address[0], address[1], address[2],
                                address[3], // Address
                                address[4], address[5], // Port
                            ])
                            .await
                        {
                            Ok(n) => {
                                println!("ACK Response sent");
                            }
                            Err(e) => {
                                println!("Error sending response");
                            }
                        }

                        // In your main function or where you set up the connections
                        let (client_reader, client_writer) =
                            io::split(socket);
                        let (server_reader, server_writer) =
                            io::split(server_socket);

                        let client_to_server =
                            tokio::spawn(bidirectional_streaming(
                                client_reader,
                                server_writer,
                            ));
                        let server_to_client =
                            tokio::spawn(bidirectional_streaming(
                                server_reader,
                                client_writer,
                            ));

                        let _ = tokio::try_join!(
                            client_to_server,
                            server_to_client
                        );
                    }
                    Err(e) => {
                        println!("Error reading from socket: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            println!("")
        }
    }
}
