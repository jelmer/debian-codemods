use std::iter::Peekable;
use std::vec::IntoIter;

#[derive(Debug, PartialEq)]
enum Token {
    True,
    False,
    Not,
    Or,
    And,
    LParen,
    RParen,
    Field(String),
    Regex(String),
    Source,
    Comparison(Comparison),
    String(String),
}

struct Lexer<'a> {
    input: &'a str,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer { input }
    }

    pub fn next_token(&mut self) -> Result<Option<Token>, String> {
        self.skip_whitespace();

        if self.input.is_empty() {
            return Ok(None);
        }

        let c = self.current_char();

        match c {
            Some('t') if self.input.starts_with("true") => {
                self.input = &self.input[4..];
                Ok(Some(Token::True))
            }
            Some('f') if self.input.starts_with("false") => {
                self.input = &self.input[5..];
                Ok(Some(Token::False))
            }
            Some('!') => {
                self.input = &self.input[1..];
                Ok(Some(Token::Not))
            }
            Some('|') => {
                self.input = &self.input[1..];
                Ok(Some(Token::Or))
            }
            Some('&') => {
                self.input = &self.input[1..];
                Ok(Some(Token::And))
            }
            Some('(') => {
                self.input = &self.input[1..];
                Ok(Some(Token::LParen))
            }
            Some(')') => {
                self.input = &self.input[1..];
                Ok(Some(Token::RParen))
            }
            Some('.') => {
                self.input = &self.input[1..];
                let field = self.consume_while(|c| c.is_alphanumeric());
                Ok(Some(Token::Field(field.to_string())))
            }
            Some('~') => {
                self.input = &self.input[1..];
                match self.current_char() {
                    Some('/') => {
                    self.input = &self.input[1..];
                    let regex = self.consume_until('/');
                    self.input = &self.input[1..];
                    Ok(Some(Token::Regex(regex.to_string())))
                    }
                    Some('"') => {
                        self.input = &self.input[1..];
                        let string = self.consume_until('"');
                        self.input = &self.input[1..];
                        Ok(Some(Token::String(string.to_string())))
                    }
                    Some(c) => Err(format!("Expected / or \" after ~, not {}", c)),
                    None => Err("Unexpected end of input".to_string()),
                }
            }
            Some('<' | '>' | '=') if self.is_comparison() => {
                let comparison = self.consume_comparison()?;
                Ok(Some(Token::Comparison(comparison)))
            }
            Some('"') => {
                self.input = &self.input[1..];
                let string = self.consume_until('"');
                self.input = &self.input[1..];
                Ok(Some(Token::String(string.to_string())))
            }
            Some('s') if self.input.starts_with("source") => {
                self.input = &self.input[6..];
                Ok(Some(Token::Source))
            }
            None => Ok(None),
            Some(c) => Err(format!("Unexpected character: {}", c)),
        }
    }

    fn current_char(&mut self) -> Option<char> {
        self.skip_whitespace();
        self.input.chars().next()
    }

    fn skip_whitespace(&mut self) {
        while !self.input.is_empty() && self.input.chars().next().unwrap().is_whitespace() {
            self.input = &self.input[1..];
        }
    }

    fn consume_while<F>(&mut self, test: F) -> &'a str
    where
        F: Fn(char) -> bool,
    {
        let mut end = 0;
        while !self.input.len() > end && test(self.input.chars().nth(end).unwrap()) {
            end += 1;
        }
        let (word, rest) = self.input.split_at(end);
        self.input = rest;
        word
    }

    fn consume_until(&mut self, end_char: char) -> &'a str {
        let mut end = 0;
        while self.input.chars().nth(end).unwrap() != end_char {
            end += 1;
        }
        let (word, rest) = self.input.split_at(end);
        self.input = rest;
        word
    }

    fn is_comparison(&self) -> bool {
        let comparisons = ["<<", "<=", "<", ">=", ">>", ">", "="];
        for &comp in &comparisons {
            if self.input.starts_with(comp) {
                return true;
            }
        }
        false
    }

    fn consume_comparison(&mut self) -> Result<Comparison, String> {
        if self.input.starts_with("<<") {
            self.input = &self.input[2..];
            Ok(Comparison::MuchLessThan)
        } else if self.input.starts_with("<=") {
            self.input = &self.input[2..];
            Ok(Comparison::LessOrEqual)
        } else if self.input.starts_with("<") {
            self.input = &self.input[2..];
            Ok(Comparison::LessThan)
        } else if self.input.starts_with(">=") {
            self.input = &self.input[2..];
            Ok(Comparison::GreaterOrEqual)
        } else if self.input.starts_with(">>") {
            self.input = &self.input[2..];
            Ok(Comparison::MuchGreaterThan)
        } else if self.input.starts_with(">") {
            self.input = &self.input[1..];
            Ok(Comparison::GreaterThan)
        } else if self.input.starts_with("=") {
            self.input = &self.input[1..];
            Ok(Comparison::Equal)
        } else {
            Err(format!("Expected comparison, got {}", self.input))
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Comparison {
    LessThan,
    MuchLessThan,
    LessOrEqual,
    GreaterThan,
    MuchGreaterThan,
    GreaterOrEqual,
    Equal,
}

impl std::fmt::Display for Comparison {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Comparison::LessThan => write!(f, "<"),
            Comparison::MuchLessThan => write!(f, "<<"),
            Comparison::LessOrEqual => write!(f, "<="),
            Comparison::GreaterThan => write!(f, ">"),
            Comparison::MuchGreaterThan => write!(f, ">>"),
            Comparison::GreaterOrEqual => write!(f, ">="),
            Comparison::Equal => write!(f, "="),
        }
    }
}

impl std::str::FromStr for Comparison {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "<" => Ok(Comparison::LessThan),
            "<<" => Ok(Comparison::MuchLessThan),
            "<=" => Ok(Comparison::LessOrEqual),
            ">" => Ok(Comparison::GreaterThan),
            ">>" => Ok(Comparison::MuchGreaterThan),
            ">=" => Ok(Comparison::GreaterOrEqual),
            "=" => Ok(Comparison::Equal),
            _ => Err(format!("Invalid comparison: {}", s)),
        }
    }
}

#[derive(PartialEq, Eq, Clone)]
enum Expr {
    /// true
    True,
    /// false
    False,
    /// !<query>
    Not(Box<Expr>),
    /// <query> | <query>
    Or(Box<Expr>, Box<Expr>),
    /// <query> & <query>
    And(Box<Expr>, Box<Expr>),
    /// Field ~ /regex/
    FieldRegex(String, String),
    /// Field ~ "string"
    FieldString(String, String),
    /// source
    Source,
    /// <comparison> "<string>"
    Comparison(Comparison, String),
    FieldComparison(String, Comparison, String),
}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Expr::True => write!(f, "True"),
            Expr::False => write!(f, "False"),
            Expr::Not(expr) => write!(f, "Not({:?})", expr),
            Expr::Or(left, right) => write!(f, "Or({:?}, {:?})", left, right),
            Expr::And(left, right) => write!(f, "And({:?}, {:?})", left, right),
            Expr::FieldRegex(field, regex) => write!(f, "FieldRegex({}, {})", field, regex),
            Expr::FieldString(field, string) => write!(f, "FieldString({}, {})", field, string),
            Expr::Source => write!(f, "Source"),
            Expr::Comparison(comp, string) => write!(f, "Comparison({}, {})", comp, string),
            Expr::FieldComparison(field, comp, string) => {
                write!(f, "FieldComparison({}, {}, {})", field, comp, string)
            }
        }
    }
}

struct Parser {
    tokens: Peekable<IntoIter<Token>>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens: tokens.into_iter().peekable(),
        }
    }

    pub fn parse(&mut self) -> Option<Expr> {
        let expr = match self.tokens.next()? {
            // true
            Token::True => Some(Expr::True),
            // false
            Token::False => Some(Expr::False),
            // ( <query> )
            Token::LParen => {
                let expr = self.parse();
                if self.tokens.next()? == Token::RParen {
                    expr
                } else {
                    None
                }
            }
            //  ! <query>
            Token::Not => Some(Expr::Not(Box::new(self.parse()?))),
            Token::Field(field) => match self.tokens.next()? {
                // <query> ~ /regex/
                Token::Regex(regex) => Some(Expr::FieldRegex(field, regex)),
                // <query> ~ "string"
                Token::String(string) => Some(Expr::FieldString(field, string)),
                // <query> ~ "<string>" <comparison> "<string>"
                Token::Comparison(comp) => {
                    if let Token::String(comp_str) = self.tokens.next()? {
                        Some(Expr::FieldComparison(field, comp, comp_str))
                    } else {
                        None
                    }
                }
                _ => None,
            },
            Token::Source => Some(Expr::Source),
            // <query> "<string>"
            Token::Comparison(comp) => {
                if let Token::String(comp_str) = self.tokens.next()? {
                    Some(Expr::Comparison(comp, comp_str))
                } else {
                    None
                }
            }
            _ => None,
        }?;

        match self.tokens.peek() {
            Some(&Token::And) => {
                self.tokens.next();
                Some(Expr::And(Box::new(expr), Box::new(self.parse()?)))
            }
            Some(&Token::Or) => {
                self.tokens.next();
                Some(Expr::Or(Box::new(expr), Box::new(self.parse()?)))
            }
            _ => Some(expr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_lex() {
        let input = r#"true & .field ~ /regex/ | .field ~ "string" << "comparison""#;
        let mut lexer = Lexer::new(input);
        let mut tokens = vec![];

        while let Some(token) = lexer.next_token().unwrap() {
            tokens.push(token);
        }

        assert_eq!(
            tokens,
            vec![
                Token::True,
                Token::And,
                Token::Field("field".to_string()),
                Token::Regex("regex".to_string()),
                Token::Or,
                Token::Field("field".to_string()),
                Token::String("string".to_string()),
                Token::Comparison(Comparison::MuchLessThan),
                Token::String("comparison".to_string())
            ]
        );
    }

    #[test]
    fn test_simple_parse() {
        let input = r#"true & .field ~ /regex/ | .field ~ "string" << "comparison""#;
        let mut lexer = Lexer::new(input);
        let mut tokens: Vec<Token> = vec![];
        while let Some(token) = lexer.next_token().unwrap() {
            tokens.push(token);
        }
        let mut parser = Parser::new(tokens);

        assert_eq!(Some(Expr::And(
            Box::new(Expr::True),
            Box::new(Expr::Or(
                Box::new(Expr::FieldRegex("field".to_string(), "regex".to_string())),
                Box::new(Expr::FieldString("field".to_string(), "string".to_string()))
            )))), parser.parse());
    }
}
