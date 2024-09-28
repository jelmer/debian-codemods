//! File parsing for the benfile format.
use std::iter::Peekable;
use std::vec::IntoIter;

#[derive(Debug, PartialEq, Clone)]
enum Token {
    True,
    False,
    Not,
    Or,
    And,
    LParen,
    RParen,
    Field(String),
    Identifier(String),
    Regex(String),
    Source,
    Comparison(Comparison),
    String(String),
    Semicolon,
    Tilde,
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
                let field = self.consume_while(Self::is_valid_field_char);
                Ok(Some(Token::Field(field.to_string())))
            }
            Some('/') => {
                self.input = &self.input[1..];
                let regex = self.consume_until('/');
                self.input = &self.input[1..];
                Ok(Some(Token::Regex(regex.to_string())))
            }
            Some('"') => {
                self.input = &self.input[1..];
                // consume until next ", but allow escaping with \
                let in_escape = std::sync::atomic::AtomicBool::new(false);
                let string = self.consume_while(move |c| {
                    if in_escape.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        true
                    } else if c == '\\' {
                        in_escape.store(true, std::sync::atomic::Ordering::SeqCst);
                        true
                    } else {
                        c != '"'
                    }
                });
                self.input = &self.input[1..];
                Ok(Some(Token::String(string.to_string())))
            }
            Some('~') => {
                self.input = &self.input[1..];
                Ok(Some(Token::Tilde))
            }
            Some('<' | '>' | '=') if self.is_comparison() => {
                let comparison = self.consume_comparison()?;
                Ok(Some(Token::Comparison(comparison)))
            }
            Some('s') if self.input.starts_with("source") => {
                self.input = &self.input[6..];
                Ok(Some(Token::Source))
            }
            Some(';') => {
                self.input = &self.input[1..];
                Ok(Some(Token::Semicolon))
            }
            Some(c) if Self::is_valid_identifier_char(c) => {
                let identifier = self.consume_while(Self::is_valid_identifier_char);
                Ok(Some(Token::Identifier(identifier.to_string())))
            }
            None => Ok(None),
            Some(c) => Err(format!("Unexpected character: {}", c)),
        }
    }

    fn is_valid_identifier_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn is_valid_field_char(c: char) -> bool {
        c.is_alphanumeric() || c == '-'
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
/// The comparison operators supported by the benfile format.
pub enum Comparison {
    /// <
    LessThan,
    /// <<
    MuchLessThan,
    /// <=
    LessOrEqual,
    /// >
    GreaterThan,
    /// >>
    MuchGreaterThan,
    /// >=
    GreaterOrEqual,
    /// =
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
/// The expression types supported by the benfile format.
pub enum Expr {
    /// true or false
    Bool(bool),
    /// !<query>
    Not(Box<Expr>),
    /// <query> | <query>
    Or(Vec<Box<Expr>>),
    /// <query> & <query>
    And(Vec<Box<Expr>>),
    /// Field ~ /regex/
    FieldRegex(String, String),
    /// Field ~ "string"
    FieldString(String, String),
    /// source
    Source,
    /// <comparison> "<string>"
    Comparison(Comparison, String),
    /// <field> ~ "<string>"
    FieldComparison(String, Comparison, String),
    /// "string"
    String(String),
}

#[derive(Debug, PartialEq, Eq)]
/// An assignment in a benfile.
pub struct Assignment {
    /// The field being assigned to.
    pub field: String,
    /// The expression being assigned.
    pub expr: Expr,
}

impl std::str::FromStr for Assignment {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lexer = Lexer::new(s);
        let mut tokens = vec![];

        while let Some(token) = lexer.next_token()? {
            tokens.push(token);
        }

        let mut parser = Parser::new(tokens);
        let assignment = parser.parse_assignment()?;
        match assignment {
            Some(assignment) => Ok(assignment),
            None => Err("Expected assignment".to_string()),
        }
    }
}

impl std::str::FromStr for Expr {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lexer = Lexer::new(s);
        let mut tokens = vec![];

        while let Some(token) = lexer.next_token()? {
            tokens.push(token);
        }

        let mut parser = Parser::new(tokens);
        parser.parse()
    }
}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Expr::Bool(b) => write!(f, "Bool({})", b),
            Expr::Not(expr) => write!(f, "Not({:?})", expr),
            Expr::Or(exprs) => write!(f, "Or({:?})", exprs),
            Expr::And(exprs) => write!(f, "And({:?})", exprs),
            Expr::FieldRegex(field, regex) => write!(f, "FieldRegex({}, {})", field, regex),
            Expr::FieldString(field, string) => write!(f, "FieldString({}, {})", field, string),
            Expr::Source => write!(f, "Source"),
            Expr::Comparison(comp, string) => write!(f, "Comparison({}, {})", comp, string),
            Expr::FieldComparison(field, comp, string) => {
                write!(f, "FieldComparison({}, {}, {})", field, comp, string)
            }
            Expr::String(string) => write!(f, "String({})", string),
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

    pub fn parse_multiple(&mut self) -> Result<Vec<Assignment>, String> {
        let mut assignments = vec![];
        while let Some(assignment) = self.parse_assignment()? {
            assignments.push(assignment);
        }
        Ok(assignments)
    }

    pub fn parse_assignment(&mut self) -> Result<Option<Assignment>, String> {
        let field = match self.tokens.next() {
            Some(Token::Identifier(field)) => field,
            None => return Ok(None),
            n => {
                return Err(format!("Expected identifier, got {:?}", n));
            }
        };
        if self.tokens.next() != Some(Token::Comparison(Comparison::Equal)) {
            return Err("Expected =".to_string());
        }
        let expr = self.parse()?;
        if self.tokens.next() == Some(Token::Semicolon) {
            Ok(Some(Assignment { field, expr }))
        } else {
            Err(format!("Expected ;, got {:?}", self.tokens.peek()))
        }
    }

    pub fn parse(&mut self) -> Result<Expr, String> {
        let expr: Expr = match self.tokens.next() {
            // true
            Some(Token::True) => Ok(Expr::Bool(true)),
            // false
            Some(Token::False) => Ok(Expr::Bool(false)),
            // "string"
            Some(Token::String(string)) => Ok(Expr::String(string)),
            // ( <query> )
            Some(Token::LParen) => {
                let expr = self.parse()?;
                match self.tokens.next() {
                    Some(Token::RParen) => Ok(expr),
                    Some(n) => Err(format!("Expected ), got {:?}", n)),
                    None => Err("Expected ), got end of file".to_string()),
                }
            }
            //  ! <query>
            Some(Token::Not) => Ok(Expr::Not(Box::new(self.parse()?))),
            Some(Token::Field(field)) => {
                if self.tokens.next() != Some(Token::Tilde) {
                    return Err(format!("Expected ~, got {:?}", self.tokens.peek()));
                }

                match self.tokens.next() {
                    // <query> ~ /regex/
                    Some(Token::Regex(regex)) => Ok(Expr::FieldRegex(field, regex)),
                    // <query> ~ "<string>" <comparison> "<string>"
                    Some(Token::String(comp_str)) => {
                        let n = self.tokens.peek().cloned();
                        match n {
                            Some(Token::Comparison(comp)) => {
                                self.tokens.next();
                                if let Some(Token::String(comp_str2)) = self.tokens.next() {
                                    Ok(Expr::FieldComparison(field, comp.clone(), comp_str2))
                                } else {
                                    Err("Expected string".to_string())
                                }
                            }
                            // <query> ~ "string"
                            _ => Ok(Expr::FieldString(field, comp_str)),
                        }
                    }
                    _ => Err(format!(
                        "Expected regex or string, got {:?}",
                        self.tokens.peek()
                    )),
                }
            }
            Some(Token::Source) => Ok(Expr::Source),
            // <query> "<string>"
            Some(Token::Comparison(comp)) => {
                if let Some(Token::String(comp_str)) = self.tokens.next() {
                    Ok(Expr::Comparison(comp, comp_str))
                } else {
                    Err("Expected string".to_string())
                }
            }
            n => Err(format!("Unexpected token: {:?}", n)),
        }?;

        match self.tokens.peek() {
            Some(&Token::And) => {
                let mut ands = vec![Box::new(expr)];
                while self.tokens.peek() == Some(&Token::And) {
                    self.tokens.next().unwrap();
                    match self.parse()? {
                        Expr::And(new_ands) => {
                            ands.extend(new_ands);
                        }
                        next_expr => {
                            ands.push(Box::new(next_expr));
                        }
                    }
                }
                Ok(Expr::And(ands))
            }
            Some(&Token::Or) => {
                let mut ors = vec![Box::new(expr)];
                while self.tokens.peek() == Some(&Token::Or) {
                    self.tokens.next().unwrap();
                    match self.parse()? {
                        Expr::Or(new_ors) => {
                            ors.extend(new_ors);
                        }
                        next_expr => {
                            ors.push(Box::new(next_expr));
                        }
                    }
                }
                Ok(Expr::Or(ors))
            }
            _ => Ok(expr),
        }
    }
}

/// Read a benfile from a reader and return a vector of assignments.
pub fn read_benfile<R: std::io::Read>(mut reader: R) -> Result<Vec<Assignment>, String> {
    let mut text = String::new();
    reader
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let mut lexer = Lexer::new(&text);
    let mut tokens = vec![];
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    let mut parser = Parser::new(tokens);
    let assignments = parser.parse_multiple()?;
    Ok(assignments)
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
                Token::Tilde,
                Token::Regex("regex".to_string()),
                Token::Or,
                Token::Field("field".to_string()),
                Token::Tilde,
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

        assert_eq!(
            Ok(Expr::And(vec![
                Box::new(Expr::Bool(true)),
                Box::new(Expr::Or(vec![
                    Box::new(Expr::FieldRegex("field".to_string(), "regex".to_string())),
                    Box::new(Expr::FieldComparison(
                        "field".to_string(),
                        Comparison::MuchLessThan,
                        "comparison".to_string()
                    ))
                ]))
            ])),
            parser.parse()
        );
    }

    #[test]
    fn test_parse_benfile() {
        let input = r###"title = "libsoup2.4 -> libsoup3";
is_affected = .build-depends ~ /libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev/ | .build-depends-arch ~ /libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev/ | .build-depends ~ /gir1.2-soup-2.4|gir1.2-soup-3.0/ | .depends ~ /gir1.2-soup-2.4/;
is_good = .depends ~ /libsoup-3.0-0|gir1.2-soup-3.0/;
is_bad = .depends ~ /libsoup-2.4-1|libsoup-gnome-2.4-1|gir1.2-soup-2.4/;
notes = "https://bugs.debian.org/cgi-bin/pkgreport.cgi?users=pkg-gnome-maintainers@lists.alioth.debian.org&tag=libsoup2";
export = false;
"###;
        let assignments = read_benfile(input.as_bytes()).unwrap();
        assert_eq!(assignments.len(), 6);
        assert_eq!(
            assignments[0],
            Assignment {
                field: "title".to_string(),
                expr: Expr::String("libsoup2.4 -> libsoup3".to_string())
            }
        );
        assert_eq!(
            assignments[1],
            Assignment {
                field: "is_affected".to_string(),
                expr: Expr::Or(vec![
                    Box::new(Expr::FieldRegex(
                        "build-depends".to_string(),
                        "libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev".to_string()
                    )),
                    Box::new(Expr::FieldRegex(
                        "build-depends-arch".to_string(),
                        "libsoup2.4-dev|libsoup-gnome2.4-dev|libsoup-3.0-dev".to_string()
                    )),
                    Box::new(Expr::FieldRegex(
                        "build-depends".to_string(),
                        "gir1.2-soup-2.4|gir1.2-soup-3.0".to_string()
                    )),
                    Box::new(Expr::FieldRegex(
                        "depends".to_string(),
                        "gir1.2-soup-2.4".to_string()
                    ))
                ])
            }
        );
        assert_eq!(assignments[4],
            Assignment {
                field: "notes".to_string(),
                expr: Expr::String("https://bugs.debian.org/cgi-bin/pkgreport.cgi?users=pkg-gnome-maintainers@lists.alioth.debian.org&tag=libsoup2".to_string())
            }
        );

        assert_eq!(
            assignments[5],
            Assignment {
                field: "export".to_string(),
                expr: Expr::Bool(false)
            }
        );
    }
}
