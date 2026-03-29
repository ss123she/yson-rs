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
        match self.lexer.peek() {
            Err(YsonError::Eof) => return Ok(false),
            Err(e) => return Err(e),
            _ => {}
        }

        if self.first_item {
            self.first_item = false;
            return Ok(true);
        }

        if self.lexer.peek()? == &Token::ItemSeparator {
            self.lexer.next_token()?;

            match self.lexer.peek() {
                Err(YsonError::Eof) => Ok(false),
                _ => Ok(true),
            }
        } else {
            Ok(true)
        }
    }

    pub fn parse_next(&mut self) -> Result<YsonValue, YsonError> {
        let attributes = if self.lexer.peek()? == &Token::BeginAttributes {
            self.lexer.next_token()?;
            Some(self.parse_map_inner(Token::EndAttributes)?)
        } else {
            None
        };

        let node = self.parse_node_inner()?;
        Ok(YsonValue { attributes, node })
    }

    fn parse_map_inner(
        &mut self,
        end_token: Token<'static>,
    ) -> Result<BTreeMap<Vec<u8>, YsonValue>, YsonError> {
        let mut map = BTreeMap::new();
        loop {
            let peeked = self.lexer.peek()?;
            if peeked == &end_token {
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

            if self.lexer.peek()? == &Token::ItemSeparator {
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
            Token::BeginMap => Ok(YsonNode::Map(self.parse_map_inner(Token::EndMap)?)),
            _ => Err(YsonError::Custom(format!("Unexpected token: {:?}", token))),
        }
    }

    fn parse_list(&mut self) -> Result<YsonNode, YsonError> {
        let mut list = Vec::new();
        loop {
            if self.lexer.peek()? == &Token::EndList {
                self.lexer.next_token()?;
                break;
            }
            list.push(self.parse_next()?);
            if self.lexer.peek()? == &Token::ItemSeparator {
                self.lexer.next_token()?;
            }
        }
        Ok(YsonNode::List(list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_list_fragment() {
        let cases = vec!["1;{a=1}", "1;{a=1};"];

        for input in cases {
            let mut parser = Parser::new(input.as_bytes(), false, StreamKind::ListFragment);

            for _ in 0..2 {
                let ok = parser.next_list_item().expect("next_list_item failed");
                assert!(ok, "Expected item to be present in: {}", input);

                let _val = parser.parse_next().expect("parse_next failed");
            }

            let ok = parser
                .next_list_item()
                .expect("Final next_list_item check failed");
            assert!(!ok, "Expected no more items in: {}", input);
        }
    }

    #[test]
    fn test_read_empty() {
        let cases = vec!["", "   "];

        for input in cases {
            let mut parser = Parser::new(input.as_bytes(), false, StreamKind::ListFragment);

            let ok = parser
                .next_list_item()
                .expect("next_list_item failed on empty");
            assert!(!ok, "Expected false for empty input: '{}'", input);
        }
    }

    #[test]
    fn test_complex_text_structure() {
        let input = r#"
    <"author"="ant0n"; "version"=1.0>{
        "users" = [
            <"id"=1>{name="Alice"; staff=%true};
            <"id"=2>{name="Bob"; staff=%false; salary=-100.5};
            #
        ]; // Wow two users! Alice and Bob! Not russian names, elimenate.
        /* 
         * Just kidding
         */
        "metadata" = <"format"="yson">{"active"=%true};
        "empty_list" = [];
        "empty_map" = {};
    }
    "#;

        let mut parser = Parser::new(input.as_bytes(), false, StreamKind::Node);
        let result = parser.parse_next().expect("Should parse complex structure");

        let attrs = result.attributes.as_ref().expect("Should have attributes");
        assert_eq!(
            attrs.get(&b"author".to_vec()).unwrap().node,
            YsonNode::String(b"ant0n".to_vec())
        );

        if let YsonNode::Map(root_map) = result.node {
            let users = &root_map.get(b"users".as_slice()).unwrap().node;
            if let YsonNode::List(user_list) = users {
                assert_eq!(user_list.len(), 3);
                let alice = &user_list[0];
                let alice_attrs = alice.attributes.as_ref().expect("Alice should have attrs");
                assert_eq!(
                    alice_attrs.get(&b"id".to_vec()).unwrap().node,
                    YsonNode::Int64(1)
                );

                let bob = &user_list[1];
                if let YsonNode::Map(bob_map) = &bob.node {
                    assert_eq!(
                        bob_map.get(&b"salary".to_vec()).unwrap().node,
                        YsonNode::Double(-100.5)
                    );
                }

                assert_eq!(user_list[2].node, YsonNode::Entity);
            } else {
                panic!("Expected list of users");
            }
        } else {
            panic!("Expected map at root");
        }
    }

    #[cfg(test)]
    mod conversion_tests {
        use super::*;

        #[test]
        fn test_yson_number_conversion_logic() {
            let cases = vec![
                ("{I=10}", true),
                ("{I=10u}", true),
                ("{U=-1}", false),
                ("{U=0.5}", false),
                ("{F=1e2}", true),
            ];

            for (input, should_pass) in cases {
                let mut parser = Parser::new(input.as_bytes(), false, StreamKind::Node);
                let res = parser.parse_next();
                if should_pass {
                    assert!(res.is_ok(), "Should have parsed: {}", input);
                }
            }
        }
    }
}
