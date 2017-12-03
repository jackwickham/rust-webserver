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


/// An error that occurs when trying to parse a request.
#[derive(Debug)]
pub struct ParseError {
    description: &'static str,
    http_response: u16,
    cause: Option<Box<Error>>,
}

impl ParseError {
    /// Create a new ParseError. It should be supplied with the description of the error and the HTTP response code
    /// that should be sent to the client.
    pub fn new(description: &'static str, http_response: u16) -> ParseError {
        ParseError {
            description,
            http_response,
            cause: None,
        }
    }

    /// Create a new ParseError from an existing error. It should be supplied with the description of the error, the
    /// HTTP response code that should be sent to the client, and the original error that caused this.
    pub fn from(description: &'static str, http_response: u16, cause: Box<Error>) -> ParseError {
        ParseError {
            description,
            http_response,
            cause: Some(cause),
        }
    }

    /// Get the HTTP response code for this error
    pub fn get_http_response(&self) -> u16 {
        self.http_response
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} (HTTP {})", self.description, self.http_response)
    }
}

impl Error for ParseError {
    fn description(&self) -> &str {
        self.description
    }

    fn cause(&self) -> Option<&Error> {
        match self.cause {
            Some(ref cause) => Some(&**cause), // Convert &Box<Error> to &Error
            None => None
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
