use std::collections::BTreeMap;

use crate::{
    error::YsonError,
    lexer::YsonIterator,
    node::{Token, YsonNode, YsonValue},
};

#[derive(Debug, PartialEq)]
pub enum StreamKind {
    Node,
    ListFragment,
}

pub struct Parser<'a> {
    lexer: YsonIterator<'a>,
    kind: StreamKind,
    first_item: bool,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a [u8], is_binary: bool, kind: StreamKind) -> Self {
        Self {
            lexer: YsonIterator::new(input, is_binary),
            kind,
            first_item: true,
        }
    }

    pub fn next_list_item(&mut self) -> Result<bool, YsonError> {
        let peek_res = self.lexer.peek_byte();
        if matches!(peek_res, Err(YsonError::Eof)) {
            return Ok(false);
        }
        let next_byte = peek_res?;

        if self.first_item {
            self.first_item = false;
            return Ok(true);
        }

        match self.kind {
            StreamKind::Node => Err(YsonError::Custom(format!(
                "Extra data at the end of YSON node: symbol '{}'",
                next_byte as char
            ))),
            StreamKind::ListFragment => {
                if next_byte == b';' {
                    self.lexer.next_token()?;

                    if matches!(self.lexer.peek_byte(), Err(YsonError::Eof)) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    pub fn parse_next(&mut self) -> Result<YsonValue, YsonError> {
        let attributes = if self.lexer.peek_byte()? == b'<' {
            self.lexer.next_token()?;
            Some(self.parse_map_inner(b'>')?)
        } else {
            None
        };

        let node = self.parse_node_inner()?;
        Ok(YsonValue { attributes, node })
    }

    fn parse_map_inner(&mut self, end_byte: u8) -> Result<BTreeMap<Vec<u8>, YsonValue>, YsonError> {
        let mut map = BTreeMap::new();
        loop {
            let peeked = self.lexer.peek_byte()?;
            if peeked == end_byte {
                self.lexer.next_token()?;
                break;
            }

            let key = match self.lexer.next_token()? {
                Token::String(s) => s.to_vec(),
                _ => return Err(YsonError::Custom("Key must be a string".into())),
            };

            if self.lexer.next_token()? != Token::KeyValueSeparator {
                return Err(YsonError::Custom("Expected '=' after map key".into()));
            }

            let value = self.parse_next()?;
            map.insert(key, value);

            if self.lexer.peek_byte()? == b';' {
                self.lexer.next_token()?;
            }
        }
        Ok(map)
    }

    fn parse_node_inner(&mut self) -> Result<YsonNode, YsonError> {
        let token = self.lexer.next_token()?;
        match token {
            Token::Entity => Ok(YsonNode::Entity),
            Token::Boolean(b) => Ok(YsonNode::Boolean(b)),
            Token::Int64(i) => Ok(YsonNode::Int64(i)),
            Token::Uint64(u) => Ok(YsonNode::Uint64(u)),
            Token::Double(d) => Ok(YsonNode::Double(d)),
            Token::String(s) => Ok(YsonNode::String(s.to_vec())),
            Token::BeginList => self.parse_list(),
            Token::BeginMap => Ok(YsonNode::Map(self.parse_map_inner(b'}')?)),
            _ => Err(YsonError::Custom(format!("Unexpected token: {:?}", token))),
        }
    }

    fn parse_list(&mut self) -> Result<YsonNode, YsonError> {
        let mut list = Vec::new();
        loop {
            if self.lexer.peek_byte()? == b']' {
                self.lexer.next_token()?;
                break;
            }
            list.push(self.parse_next()?);
            if self.lexer.peek_byte()? == b';' {
                self.lexer.next_token()?;
            }
        }
        Ok(YsonNode::List(list))
    }
}
