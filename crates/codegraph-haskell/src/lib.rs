// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Haskell parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::HaskellParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = HaskellParser::new();
        assert_eq!(parser.language(), "haskell");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = HaskellParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"module MyApp.User where

import Data.Text (Text)
import qualified Data.Map as Map

data User = User
  { userName :: Text
  , userEmail :: Text
  }

class Validatable a where
  validate :: a -> Either String a

instance Validatable User where
  validate user = Right user

createUser :: Text -> Text -> User
createUser name email = User name email

greet :: User -> Text
greet user = "Hello, " <> userName user
"#;

        let result = parser.parse_source(source, Path::new("MyApp/User.hs"), &mut graph);
        assert!(result.is_ok(), "parse failed: {:?}", result.err());

        let file_info = result.unwrap();
        // Should have found at least createUser and greet
        assert!(
            file_info.functions.len() >= 2,
            "expected >=2 functions, got {}",
            file_info.functions.len()
        );
        // Should have found User (data) and Validatable (class)
        assert!(
            file_info.classes.len() >= 2,
            "expected >=2 classes, got {}",
            file_info.classes.len()
        );
        // Should have found 2 imports
        assert_eq!(file_info.imports.len(), 2);
    }
}
