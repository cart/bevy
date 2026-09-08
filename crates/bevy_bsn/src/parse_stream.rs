use std::{collections::VecDeque, fmt::Display};

use thiserror::Error;

use crate::{
    lex::{LexError, SpannedToken, Token},
    span::{OwnedSpan, Span},
};

#[derive(Error)]
pub enum ParseError<'a> {
    #[error(transparent)]
    LexError(LexError),
    #[error("{}", .0.span.message(&format!("Unexpected Token {:?}", .0.token)))]
    UnexpectedToken(SpannedToken<'a>),
    #[error("Unexpected end of input.")]
    EndOfInput,
    #[error("{}", .token.span.message(&.message))]
    SpannedMessage {
        message: String,
        token: SpannedToken<'a>,
    },
    #[error("{0}")]
    Message(String),
}

#[derive(Error)]
pub enum OwnedParseError {
    #[error(transparent)]
    LexError(LexError),
    #[error("{}", .0.span.as_span().message(&format!("Unexpected Token {:?}", .0.token)))]
    UnexpectedToken(OwnedSpannedToken),
    #[error("Unexpected end of input.")]
    EndOfInput,
    #[error("{}", .token.span.as_span().message(&.message))]
    SpannedMessage {
        message: String,
        token: OwnedSpannedToken,
    },
    #[error("{0}")]
    Message(String),
}

impl<'a> From<ParseError<'a>> for OwnedParseError {
    fn from(value: ParseError<'a>) -> Self {
        match value {
            ParseError::LexError(lex_error) => OwnedParseError::LexError(lex_error),
            ParseError::UnexpectedToken(spanned_token) => {
                OwnedParseError::UnexpectedToken(spanned_token.into())
            }
            ParseError::EndOfInput => OwnedParseError::EndOfInput,
            ParseError::SpannedMessage { message, token } => OwnedParseError::SpannedMessage {
                message,
                token: token.into(),
            },
            ParseError::Message(message) => OwnedParseError::Message(message),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OwnedSpannedToken {
    pub token: Token,
    pub span: OwnedSpan,
}

impl<'a> From<SpannedToken<'a>> for OwnedSpannedToken {
    fn from(value: SpannedToken<'a>) -> Self {
        Self {
            token: value.token,
            span: value.span.into(),
        }
    }
}

impl<'a> std::fmt::Debug for ParseError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

impl std::fmt::Debug for OwnedParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}
impl<'a> From<LexError> for ParseError<'a> {
    fn from(value: LexError) -> Self {
        Self::LexError(value)
    }
}

pub struct ParseStream<'a> {
    span: Span<'a>,
    peek_buffer: VecDeque<Result<SpannedToken<'a>, LexError>>,
}

impl<'a> From<Span<'a>> for ParseStream<'a> {
    fn from(value: Span<'a>) -> Self {
        Self {
            span: value,
            peek_buffer: VecDeque::new(),
        }
    }
}

impl<'a> ParseStream<'a> {
    pub fn parse<P: Parse>(&mut self) -> Result<P, ParseError<'a>> {
        P::parse(self)
    }

    pub fn peek<P: Peek>(&mut self) -> bool {
        P::peek(self)
    }

    pub fn is_empty(&mut self) -> bool {
        self.peek_token().is_none()
    }

    pub fn is_empty_or_closing_delimiter(&mut self) -> bool {
        if let Some(token) = self.peek_token() {
            match token.token {
                Token::RBracket | Token::RParen | Token::RBrace => true,
                _ => false,
            }
        } else {
            true
        }
    }

    pub fn peek_token(&mut self) -> Option<&SpannedToken<'a>> {
        if !self.peek_buffer.is_empty() {
            self.peek_buffer.front().unwrap().as_ref().ok()
        } else {
            match self.span.next() {
                Some(result) => {
                    self.peek_buffer.push_back(result);
                    self.peek_buffer.back().unwrap().as_ref().ok()
                }
                None => None,
            }
        }
    }

    pub fn next(&mut self) -> Result<Option<SpannedToken<'a>>, LexError> {
        if let Some(value) = self.peek_buffer.pop_front() {
            return value.map(|v| Some(v));
        }

        self.span.next().transpose()
    }

    pub fn error(&mut self, message: impl Into<String>) -> ParseError<'a> {
        match self.peek_token() {
            Some(token) => ParseError::SpannedMessage {
                message: message.into(),
                token: token.clone(),
            },
            None => ParseError::Message(message.into()),
        }
    }

    pub fn bracketed<P: Parse>(&mut self) -> Result<P, ParseError<'a>> {
        self.parse::<LBracket>()?;
        let p = self.parse::<P>()?;
        self.parse::<RBracket>()?;
        Ok(p)
    }

    pub fn parenthesized<P: Parse>(&mut self) -> Result<P, ParseError<'a>> {
        self.parse::<LParen>()?;
        let p = self.parse::<P>()?;
        self.parse::<RParen>()?;
        Ok(p)
    }

    pub fn braced<P: Parse>(&mut self) -> Result<P, ParseError<'a>> {
        self.parse::<LBrace>()?;
        let p = self.parse::<P>()?;
        self.parse::<RBrace>()?;
        Ok(p)
    }
}

pub trait Parse: Sized {
    fn parse<'a>(input: &mut ParseStream<'a>) -> Result<Self, ParseError<'a>>;
}

pub trait Peek {
    fn peek(input: &mut ParseStream) -> bool;
}

macro_rules! impl_parse_token {
    ($token:ident) => {
        impl Parse for $token {
            fn parse<'a>(input: &mut ParseStream<'a>) -> Result<Self, ParseError<'a>> {
                match input.next()? {
                    Some(token) => {
                        if matches!(token.token, Token::$token) {
                            Ok($token)
                        } else {
                            Err(ParseError::UnexpectedToken(token))
                        }
                    }
                    None => Err(ParseError::EndOfInput),
                }
            }
        }

        impl Peek for $token {
            fn peek(input: &mut ParseStream) -> bool {
                if let Some(token) = input.peek_token() {
                    matches!(token.token, Token::$token)
                } else {
                    false
                }
            }
        }
    };
}

macro_rules! impl_parse_arg_token {
    ($token:ident) => {
        impl Parse for $token {
            fn parse<'a>(input: &mut ParseStream<'a>) -> Result<Self, ParseError<'a>> {
                match input.next()? {
                    Some(token) => {
                        if let Token::$token(value) = token.token {
                            Ok($token(value))
                        } else {
                            Err(ParseError::UnexpectedToken(token))
                        }
                    }
                    None => Err(ParseError::EndOfInput),
                }
            }
        }

        impl Peek for $token {
            fn peek(input: &mut ParseStream) -> bool {
                if let Some(token) = input.peek_token() {
                    matches!(token.token, Token::$token(_))
                } else {
                    false
                }
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bool(pub bool);
impl_parse_arg_token!(Bool);

// PERF: Consider borrowing this from the source input
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident(pub String);
impl_parse_arg_token!(Ident);

impl From<&str> for Ident {
    fn from(value: &str) -> Self {
        Ident(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLit(pub String);
impl_parse_arg_token!(StringLit);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Int(pub i128);
impl_parse_arg_token!(Int);

#[derive(Debug, Clone, PartialEq)]
pub struct Float(pub f64);
impl_parse_arg_token!(Float);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LBracket;
impl_parse_token!(LBracket);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RBracket;
impl_parse_token!(RBracket);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LParen;
impl_parse_token!(LParen);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RParen;
impl_parse_token!(RParen);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LBrace;
impl_parse_token!(LBrace);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RBrace;
impl_parse_token!(RBrace);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comma;
impl_parse_token!(Comma);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoubleColon;
impl_parse_token!(DoubleColon);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoubleMinus;
impl_parse_token!(DoubleMinus);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colon;
impl_parse_token!(Colon);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hash;
impl_parse_token!(Hash);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct At;
impl_parse_token!(At);

#[cfg(test)]
mod tests {
    use crate::{
        parse_stream::{At, ParseError, ParseStream},
        span::Span,
    };

    #[test]
    fn parse_basics() {
        let mut input = ParseStream::from(Span::from("@"));
        assert!(input.peek::<At>());
        assert!(!input.is_empty());
        assert!(input.peek::<At>());
        let _ = input.parse::<At>().unwrap();
        assert!(matches!(input.parse::<At>(), Err(ParseError::EndOfInput)));
        assert!(input.is_empty());
    }
}
