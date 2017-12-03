pub mod http;

use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::error::Error;

use http::request::Request;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();

    for stream in listener.incoming() {
        let stream = stream.unwrap();

        handle_connection(stream);
    }
}

fn handle_connection(mut stream: TcpStream) {
    match Request::from(&mut stream) {
        Ok(d) => process_request(&mut stream, d),
        Err(e) => {
            println!("{}", e.description());
            send_error(&mut stream, e.get_http_response());
        }
    };

    stream.flush().unwrap();
}

fn send_error(stream: &mut TcpStream, response_code: u16) {
    let headers = format!("HTTP/1.1 {} GENERIC ERROR", response_code);
    let body = format!("<h1>Error</h1><p>{}</p>", response_code);
    
    let response = format!("{}\r\n\r\n{}", headers, body);
    stream.write(response.as_bytes()).unwrap();
}

fn process_request(stream: &mut TcpStream, req: Request) {
    let headers = "HTTP/1.1 200 OK";
    let mut body = format!("<h1>Success</h1><p>Requested {}</p><h2>Headers</h1>", req.get_target());
    for header in req.get_headers() {
        body = format!("{}<p><b>{}</b>: {}", body, header.0, header.1);
    }
    
    let response = format!("{}\r\n\r\n{}", headers, body);
    stream.write(response.as_bytes()).unwrap();
}