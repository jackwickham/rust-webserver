//! A private utility module for request parsing

use std::io::prelude::*;
use std::error::Error;
use std::fmt;

/// An iterator that wraps around a borrowed struct that implements [`std::io::Read`]. This is needed because the
/// default iterators in `std::io` take ownership of the reader, but we want to be able to write to it later too.
pub struct StreamReader<'a, T: Read + 'a> {
    stream: &'a mut T,
    buffer: [u8; 1024],
    index: usize,
    read: usize,
}

impl<'a, T: Read + 'a> StreamReader<'a, T> {
    /// Create a new `StreamReader` from a reader
    pub fn from(stream: &'a mut T) -> StreamReader<'a, T> {
        StreamReader {
            stream: stream,
            buffer: [0; 1024],
            index: 0,
            read: 0,
        }
    }

    fn read(&mut self) -> bool {
        self.read = match self.stream.read(&mut self.buffer) {
            Ok(n) => n,
            Err(_) => return false,
        };
        self.index = 0;
        true
    }

    /// Decrement the iterator, so the next call to `next` will return the previous value again. Returns `true` if the
    /// decrement was successful, and `false` if it failed.
    /// 
    /// This method will succceed the first time it is called after `next`, if that call to `next` returned `Some`.
    /// Further invocations may succeed, depending on the current state of the buffer.
    /// 
    /// # Examples
    /// ```
    /// let before = reader.next();
    /// reader.step_back().unwrap();
    /// let after = reader.next();
    /// assert_eq!(before, after);
    /// ```
    /// The following example may fail
    /// ```
    /// let before = reader.next();
    /// reader.step_back().unwrap(); // fine - we just called next
    /// reader.step_back().unwrap(); // may fail depending on the internal state of reader
    /// ```
    pub fn step_back(&mut self) -> Option<()> {
        if self.index > 0 {
            self.index -= 1;
            Some(())
        } else {
            None
        }
    }
}

impl<'a, T: Read + 'a> Iterator for StreamReader<'a, T> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.read {
            if !self.read() {
                return None;
            }
            // If we didn't read anything this time, we're done
            if self.index >= self.read {
                return None;
            }
        }

        let result = Some(self.buffer[self.index]);
        self.index += 1;

        result
    }
}



/// An error that occurred when trying to parse the request
#[derive(Debug)]
pub enum ParseError {
    EOF,
    IllegalCharacter,
    Generic {err: Box<Error>, http_response: u16},
}

impl ParseError {
    /// Create a new generic error from anything that can be converted into an error (including &str).
    ///
    /// The HTTP response code that should be sent also needs to be provided
    pub fn new_generic<E>(err: E, http_response: u16) -> ParseError
        where E: Into<Box<Error>>
    {
        ParseError::Generic {
            err: err.into(),
            http_response,
        }
    }

    /// Create a new generic error from anything that can be converted into an error (including &str), and return error
    /// 400 Bad Request to the client
    pub fn new_bad_request<E>(err: E) -> ParseError
        where E: Into<Box<Error>>
    {
        ParseError::new_generic(err, 400)
    }

    /// Get the HTTP response code that should be sent to the client.
    /// 
    /// Returns None if the connection should be closed with no response sent.
    pub fn http_response_code(&self) -> Option<u16> {
        match self {
            &ParseError::EOF => None,
            &ParseError::IllegalCharacter => Some(400),
            &ParseError::Generic {http_response: r, ..} => Some(r),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.http_response_code() {
            Some(c) => write!(f, "{} (HTTP {})", self.description(), c),
            None => write!(f, "{} (no response sent to client)", self.description()),
        }
    }
}

impl Error for ParseError {
    fn description(&self) -> &str {
        match self {
            &ParseError::EOF => "End of file reached while parsing headers",
            &ParseError::IllegalCharacter => "Illegal character encountered while parsing headers",
            &ParseError::Generic {ref err, ..} => err.description(),
        }
    }
}


/// A wrapper for parsing `token` as defined in [RFC 7230 Appendix B](https://tools.ietf.org/html/rfc7230#appendix-B).
/// 
/// Tokens are used in lots of places in the header, so this abstracts the parsing away. A valid token is a sequence of
/// TChars.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TokenType {
    // tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
    TChar(u8),
    Invalid(u8),
}

impl TokenType {
    pub fn from(c: u8) -> TokenType {
        match c {
            b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' |
                b'0'...b'9' | b'a'...b'z' | b'A'...b'Z' => TokenType::TChar(c),
            c => TokenType::Invalid(c),
        }
    }
}

unsafe impl Send for TokenType {}
unsafe impl Sync for TokenType {}
