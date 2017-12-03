//! # request
//! [RFC 7230](https://tools.ietf.org/html/rfc723) compliant HTTP 1.1 request parser

pub mod util;

use std::io::prelude::*;
use std::net::TcpStream;
use std::collections::HashMap;
use std::sync::Arc;

use self::util::*;
pub use self::util::ParseError;

/// A container for the details of an HTTP request
#[derive(Debug, Eq, PartialEq)]
pub struct Request {
    /// HTTP Version
    version: (u8, u8),
    /// HTTP Method (verb)
    method: Method,
    /// Target (the URI from path onwards)
    target: String,
    /// The HTTP request headers
    headers: HashMap<String, String>,
    /// The request body
    body: Option<Vec<u8>>,
}

impl Request {
    /// Get the request's HTTP version, in the format (major, minor)
    pub fn get_version(&self) -> (u8, u8) {
        self.version
    }
    /// Get the request method
    pub fn get_method(&self) -> &Method {
        &self.method
    }
    /// Get the request target (usually the [origin form](https://tools.ietf.org/html/rfc7230#section-5.3.1) of the
    /// request url, which is the absolute path followed optionally by the query)
    pub fn get_target(&self) -> &str {
        &self.target
    }
    /// Get the request headers
    /// TODO: This should either be a collection or parsed to combine comma separated headers
    pub fn get_headers(&self) -> &HashMap<String, String> {
        &self.headers
    }
    /// Get the request body, if one was supplied in the request
    pub fn get_body(&self) -> Option<&[u8]> {
        match self.body {
            Some(ref d) => Some(d),
            None => None,
        }
    }
}

impl Request {
    /// Parse a request stream
    pub fn from(stream: &mut TcpStream) -> Result<Request, ParseError> {
        let mut builder = RequestBuilder::new();
        let mut it = StreamReader::from(stream);

        Request::parse_request_line(&mut builder, &mut it)?;

        Ok(builder.into_request().unwrap())
    }

    /// Parse the request line, which is the first line of the request
    /// 
    /// It should have the form `Method Target HTTP/Version`, as defined in
    /// [RFC 7230 §3.1.1](https://tools.ietf.org/html/rfc7230#section-3.1.1).
    fn parse_request_line<T>(builder: &mut RequestBuilder, it: &mut StreamReader<T>) -> Result<(), ParseError>
        where T: Read {
        // Request method
        let method = Request::parse_request_method(it)?;
        builder.set_method(method);

        // Target
        let target = Request::parse_request_target(it)?;
        builder.set_target(target);

        // Version
        let version = Request::parse_request_version(it)?;
        builder.set_version(version.0, version.1);

        Ok(())
    }

    /// Parse the method (GET, POST, etc). It should be 1 or more visible characters, treated case-sensitively, and it
    /// is followed by a single space (according to
    /// [RFC 7230 §3.1.1](https://tools.ietf.org/html/rfc7230#section-3.1.1)).
    fn parse_request_method<T>(it: &mut StreamReader<T>) -> Result<Method, ParseError>
        where T: Read {
        let mut method = Vec::new();
        // Read bytes
        loop {
            match it.next() {
                Some(b' ') => break,
                Some(13) | Some(10) =>  // new lines aren't allowed
                    return Err(ParseError::new("Unexpected character when parsing request method", 400)),
                Some(b) => method.push(b),
                None => return Err(ParseError::new("Unexpected end of stream when parsing request method", 400)),
            }
        }

        // Convert it to method type and store it
        Ok(Method::from(method))
    }

    /// Parse the target (requested resource). The most general form is 1 or more visible characters (followed by a
    /// single space), though more restrictive parsing would be permitted as defined in
    /// [RFC 7230 §5.3](https://tools.ietf.org/html/rfc7230#section-5.3).
    fn parse_request_target<T>(it: &mut StreamReader<T>) -> Result<String, ParseError>
        where T: Read {
        let mut target = Vec::new();
        // Read bytes
        loop {
            match it.next() {
                Some(b' ') => break,
                Some(13) | Some(10) =>  // new lines aren't allowed
                    return Err(ParseError::new("Unexpected character when parsing request target", 400)),
                Some(b) => target.push(b),
                None => return Err(ParseError::new("Unexpected end of stream when parsing request target", 400)),
            }
        }

        match String::from_utf8(target) {
            Ok(s) => Ok(s),
            Err(e) => Err(ParseError::from("Invalid unicode when parsing request target", 400, Box::from(e))),
        }
    }

    /// Parse the HTTP version, which should be HTTP/maj.min, where maj and min are single digits, as defined in
    // [RFC 7230 §2.6](https://tools.ietf.org/html/rfc7230#section-2.6).
    fn parse_request_version<T>(it: &mut StreamReader<T>) -> Result<(u8, u8), ParseError>
        where T: Read {
        let expected_it = "HTTP/".bytes();
        for expected in expected_it {
            match it.next() {
                Some(b) if b == expected => (),
                Some(_) => return Err(ParseError::new("Unexpected character when parsing request version", 400)),
                None => return Err(ParseError::new("Unexpected end of stream when parsing request version", 400)),
            }
        }
        let major = match it.next() {
            Some(n) if n >= 48 && n <= 57 => n - 48,
            Some(_) => return Err(ParseError::new("Unexpected character when parsing request version", 400)),
            None => return Err(ParseError::new("Unexpected end of stream when parsing request version", 400)),
        };
        match it.next() {
            Some(b'.') => (),
            Some(_) => return Err(ParseError::new("Unexpected character when parsing request version", 400)),
            None => return Err(ParseError::new("Unexpected end of stream when parsing request version", 400)),
        }
        let minor = match it.next() {
            Some(n) if n >= 48 && n <= 57 => n - 48,
            Some(_) => return Err(ParseError::new("Unexpected character when parsing request version", 400)),
            None => return Err(ParseError::new("Unexpected end of stream when parsing request version", 400)),
        };
        
        // Should now be at the end of the Request Line
        match it.next() {
            Some(b'\r') => (),
            Some(_) => return Err(ParseError::new("Unexpected character when parsing request version", 400)),
            None => return Err(ParseError::new("Unexpected end of stream when parsing request version", 400)),
        }
        match it.next() {
            Some(b'\n') => (),
            Some(_) => return Err(ParseError::new("Unexpected character when parsing request version", 400)),
            None => return Err(ParseError::new("Unexpected end of stream when parsing request version", 400)),
        }

        Ok((major, minor))
    }
}

unsafe impl Send for Request {}

// nb. Not syncable until fully constructed (when the hashmap becomes effectively immutable)
// Public interface is completely syncable
unsafe impl Sync for Request {}



/// HTTP Methods (verbs), as defined by [RFC 7231 §4](https://tools.ietf.org/html/rfc7231#section-4)
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
    Put,
    Head,
    Connect,
    Options,
    Trace,
    Custom(Arc<Vec<u8>>),
}

impl Method {
    /// Construct a `Method` from the corresponding case-sensitive name, provided as a vector of bytes.
    /// Ownership of the vector is required to store the name in the event that it isn't a known method.
    pub fn from(name: Vec<u8>) -> Method {
        use self::Method::*;

        if name.as_slice() == &b"GET"[..] { return Get };
        if name.as_slice() == &b"POST"[..] { return Post };
        if name.as_slice() == &b"PATCH"[..] { return Patch };
        if name.as_slice() == &b"DELETE"[..] { return Delete };
        if name.as_slice() == &b"PUT"[..] { return Put };
        if name.as_slice() == &b"HEAD"[..] { return Head };
        if name.as_slice() == &b"CONNECT"[..] { return Connect };
        if name.as_slice() == &b"OPTIONS"[..] { return Options };
        if name.as_slice() == &b"TRACE"[..] { return Trace };
        return Custom(Arc::from(name));
    }
}

unsafe impl Send for Method {}


/// TODO: Docs
#[derive(Debug, Eq, PartialEq)]
struct RequestBuilder {
    version: Option<(u8, u8)>,
    method: Option<Method>,
    target: Option<String>,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
}

impl RequestBuilder {
    pub fn new() -> RequestBuilder {
        RequestBuilder {
            version: None,
            method: None,
            target: None,
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn set_version(&mut self, major: u8, minor: u8) {
        self.version = Some((major, minor));
    }

    pub fn set_method(&mut self, method: Method) {
        self.method = Some(method);
    }

    pub fn set_target(&mut self, target: String) {
        self.target = Some(target);
    }

    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = Some(body);
    }

    pub fn add_header(&mut self, key: String, val: String) {
        self.headers.insert(key, val);
    }

    pub fn into_request(self) -> Option<Request> {
        match self {
            RequestBuilder {
                version: Some(version),
                method: Some(method),
                target: Some(target),
                headers,
                body,
            } => Some(Request{
                version, method, target, headers, body
            }),
            _ => None,
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::str::Bytes;
    use std::io;

    #[test]
    fn test_parse_request_line() {
        let mut builder = RequestBuilder::new();
        let mut byte_iterator = StrReader::new("GET /test/path?k=v&k2 HTTP/1.1\r\n".bytes());
        let mut it = StreamReader::from(&mut byte_iterator);

        Request::parse_request_line(&mut builder, &mut it).unwrap();

        assert_eq!(builder, RequestBuilder {
            version: Some((1, 1)),
            method: Some(Method::Get),
            target: Some(String::from("/test/path?k=v&k2")),
            headers: HashMap::new(),
            body: None,
        });
    }

    struct StrReader<'a> {
        data: Bytes<'a>,
    }

    impl<'a> StrReader<'a> {
        fn new(data: Bytes) -> StrReader {
            StrReader {
                data,
            }
        }
    }

    impl<'a> Read for StrReader<'a> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
            let len = buf.len();
            let mut i = 0;
            while i < len {
                buf[i] = match self.data.next() {
                    Some(d) => d,
                    None => return Ok(i),
                };
                i += 1;
            }
            Ok(i)
        }
    }
}