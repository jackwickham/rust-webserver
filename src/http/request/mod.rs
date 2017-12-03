//! [RFC 7230](https://tools.ietf.org/html/rfc723) compliant HTTP 1.1 request parser

mod util;

use std::io::prelude::*;
use std::net::TcpStream;
use std::collections::HashMap;
use std::sync::Arc;

use self::util::*;
pub use self::util::ParseError;
use self::util::TokenType::{TChar, Invalid};

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
        Request::parse_headers(&mut builder, &mut it)?;

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
        for b in it {
            match TokenType::from(b) {
                TChar(c) => method.push(c),
                Invalid(b' ') => return Ok(Method::from(method)),
                Invalid(_) => return Err(ParseError::IllegalCharacter),
            }
        }

        Err(ParseError::EOF)
    }

    /// Parse the target (requested resource). The most general form is 1 or more visible characters (followed by a
    /// single space), though more restrictive parsing would be permitted as defined in
    /// [RFC 7230 §5.3](https://tools.ietf.org/html/rfc7230#section-5.3).
    fn parse_request_target<T>(it: &mut StreamReader<T>) -> Result<String, ParseError>
        where T: Read {
        let mut target = Vec::new();
        // Read bytes
        for b in it {
            match b {
                // Allowed characters in URLs per [RFC 3986](https://tools.ietf.org/html/rfc3986#appendix-A)
                b'!' | b'#'...b';' | b'=' | b'?'...b'[' | b']'...b'z' | b'|' | b'~' => target.push(b),
                b' ' => return Ok(String::from_utf8(target).unwrap()), // Safe to unwrap because input is sanitised
                _ => return Err(ParseError::IllegalCharacter),
            }
        }

        Err(ParseError::EOF)
    }

    /// Parse the HTTP version, which should be HTTP/maj.min, where maj and min are single digits, as defined in
    /// [RFC 7230 §2.6](https://tools.ietf.org/html/rfc7230#section-2.6).
    fn parse_request_version<T>(it: &mut StreamReader<T>) -> Result<(u8, u8), ParseError>
        where T: Read {

        let expected_it = "HTTP/".bytes();

        for expected in expected_it {
            match it.next() {
                Some(b) if b == expected => (),
                Some(_) => return Err(ParseError::IllegalCharacter),
                None => return Err(ParseError::EOF),
            }
        }
        let major = match it.next() {
            Some(n) if n >= 48 && n <= 57 => n - 48,
            Some(_) => return Err(ParseError::IllegalCharacter),
            None => return Err(ParseError::EOF),
        };
        match it.next() {
            Some(b'.') => (),
            Some(_) => return Err(ParseError::IllegalCharacter),
            None => return Err(ParseError::EOF),
        }
        let minor = match it.next() {
            Some(n) if n >= 48 && n <= 57 => n - 48,
            Some(_) => return Err(ParseError::IllegalCharacter),
            None => return Err(ParseError::EOF),
        };
        
        // Should now be at the end of the Request Line
        match it.next() {
            Some(b'\r') => (),
            Some(_) => return Err(ParseError::IllegalCharacter),
            None => return Err(ParseError::EOF),
        }
        match it.next() {
            Some(b'\n') => (),
            Some(_) => return Err(ParseError::IllegalCharacter),
            None => return Err(ParseError::EOF),
        }

        Ok((major, minor))
    }

    /// Parse the request headers from `it` into `builder`, as specified in
    /// [RFC 7230 §3.2](https://tools.ietf.org/html/rfc7230#section-3.2)
    fn parse_headers<T: Read>(builder: &mut RequestBuilder, it: &mut StreamReader<T>) -> Result<(), ParseError> {
        // An enum to store the current state of the parser
        enum ParserState {
            // After a new line, ready to parse the header name
            Start,
            // Currently parsing the header name
            Name {name: Vec<u8>},
            // Currently parsing the whitespace after the : but before the value
            ValueLeadingWS {name: String},
            // Currently parsing the value
            Value {name: String, value: Vec<u8>},
            // Currently parsing the new line (CR (here) LF)
            NewLine,
            // Currently parsing the final new line (CR LF CR (here) LF)
            FinalNewLine,
        };
        let mut state = ParserState::Start;

        'outer: loop {
            let b = match it.next() {
                None => return Err(ParseError::EOF),
                Some(b) => b,
            };

            // Wrap this in a loop so that we can cheaply transition to a different state without having consumed
            // any characters
            loop {
                match state {
                    ParserState::Start => match b {
                        b'\r' => state = ParserState::FinalNewLine,
                        _ => {
                            // Move straight into Name without consuming this character
                            state = ParserState::Name {
                                name: Vec::new()
                            };
                            continue;
                        }
                    },
                    ParserState::Name {name: mut n} => match TokenType::from(b) {
                        TChar(c) => {
                            n.push(c);
                            state = ParserState::Name {name: n}
                        },
                        Invalid(b':') => {
                            // Safe to convert to UTF-8 because it was constructed from just ASCII characters
                            let name = String::from_utf8(n).unwrap();
                            state = ParserState::ValueLeadingWS {name: name};
                        },
                        Invalid(_) => return Err(ParseError::IllegalCharacter),
                    },
                    ParserState::ValueLeadingWS {name: n} => match b {
                        b' ' | b'\t' => state = ParserState::ValueLeadingWS {name: n},
                        _ => {
                            // Move straight into Value without consuming
                            state = ParserState::Value {
                                name: n,
                                value: Vec::new()
                            };
                            continue;
                        }
                    },
                    ParserState::Value {name: n, value: mut v} => match b {
                        b'\t' | b' '...b'~' => {
                            v.push(b);
                            state = ParserState::Value {name: n, value: v};
                        },
                        0x80...0xFF => {
                            // The specification says that headers containing these characters SHOULD be considered as
                            // opaque data. However, doing that means we can't treat the headers as strings, because
                            // this would break UTF-8 compliance, thereby vastly increasing the complexity of the rest
                            // of the code. The non-ASCII characters will therefore be silently discarded
                            state = ParserState::Value {name: n, value: v};
                        }
                        b'\r' => {
                            // Because we discarded the invalid characters, it's safe to convert to UTF-8
                            let value = String::from_utf8(v).unwrap();
                            // Store the header
                            builder.add_header(n, value);
                            // Transition to expect the LF
                            state = ParserState::NewLine;
                        },
                        _ => return Err(ParseError::IllegalCharacter),
                    },
                    ParserState::NewLine => match b {
                        b'\n' => state = ParserState::Start,
                        _ => return Err(ParseError::IllegalCharacter),
                    },
                    ParserState::FinalNewLine => match b {
                        b'\n' => break 'outer,
                        _ => return Err(ParseError::IllegalCharacter),
                    }
                }

                // Consume the next character
                break;
            }
        }

        Ok(())
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


/// A struct that can be used to incrementally build up a request, so the components are optional
#[derive(Debug, Eq, PartialEq)]
struct RequestBuilder {
    version: Option<(u8, u8)>,
    method: Option<Method>,
    target: Option<String>,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
}

impl RequestBuilder {
    /// Construct a new RequestBuilder
    pub fn new() -> RequestBuilder {
        RequestBuilder {
            version: None,
            method: None,
            target: None,
            headers: HashMap::new(),
            body: None,
        }
    }

    /// Set the HTTP version of this request
    pub fn set_version(&mut self, major: u8, minor: u8) {
        self.version = Some((major, minor));
    }

    /// Set the request method
    pub fn set_method(&mut self, method: Method) {
        self.method = Some(method);
    }

    /// Set the request target
    pub fn set_target(&mut self, target: String) {
        self.target = Some(target);
    }

    /// Set the body of the request
    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = Some(body);
    }

    /// Add a header. This method currently stores the latest version in the event of duplicate headers.
    pub fn add_header(&mut self, key: String, val: String) {
        self.headers.insert(key, val);
    }

    /// Convert this request builder into a full request
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