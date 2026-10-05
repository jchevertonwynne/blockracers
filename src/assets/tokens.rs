//! The binary token stream shared by every `.?DB` / `.??B` file. It is a tokenised form
//! of a text format: strings, numbers, brackets and per-format keywords, with run-length
//! arrays and user-defined token sequences ("structs") for compactness.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Str(String),
    Float(f32),
    Int(i32),
    LCurly,
    RCurly,
    LBracket,
    RBracket,
    Key(u16),
}

struct Lexer<'a> {
    d: &'a [u8],
    at: usize,
    /// Token sequences defined with 0x16, expanded by tokens 0x17..=0x26.
    sequences: [Vec<u16>; 16],
    out: Vec<Token>,
}

impl Lexer<'_> {
    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let b = self.d.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(b)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes::<1>()?[0])
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_le_bytes(self.bytes()?))
    }

    /// A token code, where 0x13 escapes a 16-bit code.
    fn code(&mut self) -> Option<u16> {
        match self.u8()? {
            0x13 => Some(u16::from_le_bytes(self.bytes()?)),
            c => Some(c as u16),
        }
    }

    fn token(&mut self, code: u16) -> Option<()> {
        let token = match code {
            0x02 => {
                let len = self.d[self.at..].iter().position(|&b| b == 0)?;
                let s = String::from_utf8_lossy(&self.d[self.at..self.at + len]).into_owned();
                self.at += len + 1;
                Token::Str(s)
            }
            0x03 => Token::Float(f32::from_le_bytes(self.bytes()?)),
            0x04 => Token::Int(i32::from_le_bytes(self.bytes()?)),
            0x05 => Token::LCurly,
            0x06 => Token::RCurly,
            0x07 => Token::LBracket,
            0x08 => Token::RBracket,
            0x0b | 0x0c => Token::Int(self.u8()? as i32),
            0x0d => Token::Int(self.i16()? as i32),
            0x0e => Token::Int(self.i16()? as u16 as i32),
            0x0f => Token::Float(self.i16()? as f32 / 4096.0),
            0x10 => Token::Float(self.i16()? as f32 / 32.0),
            0x11 => Token::Float(self.i16()? as f32),
            0x12 => Token::Float(self.u8()? as f32 / 127.0),
            0x14 => {
                let count = self.i16()? as u16;
                let code = self.code()?;
                for _ in 0..count {
                    self.token(code)?;
                }
                return Some(());
            }
            0x15 => {
                for code in [0x07, 0x04, 0x08, 0x08] {
                    self.token(code)?;
                }
                return Some(());
            }
            0x16 => {
                let index = self.u8()?.checked_sub(0x17)? as usize;
                let count = self.u8()?;
                let sequence = (0..count)
                    .map(|_| self.code())
                    .collect::<Option<Vec<_>>>()?;
                *self.sequences.get_mut(index)? = sequence;
                return Some(());
            }
            0x17..=0x26 => {
                for code in self.sequences[code as usize - 0x17].clone() {
                    self.token(code)?;
                }
                return Some(());
            }
            key => Token::Key(key),
        };
        self.out.push(token);
        Some(())
    }
}

/// Tokenises a whole file; a truncated or corrupt tail is dropped.
pub fn tokenize(d: &[u8]) -> Vec<Token> {
    let mut lexer = Lexer {
        d,
        at: 0,
        sequences: Default::default(),
        out: Vec::new(),
    };
    while let Some(code) = lexer.code() {
        if lexer.token(code).is_none() {
            break;
        }
    }
    lexer.out
}

pub struct Reader {
    tokens: Vec<Token>,
    at: usize,
}

impl Reader {
    pub fn new(data: &[u8]) -> Self {
        Reader {
            tokens: tokenize(data),
            at: 0,
        }
    }

    pub fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    pub fn int(&mut self) -> Option<i32> {
        match self.next()? {
            Token::Int(v) => Some(v),
            Token::Float(v) => Some(v as i32),
            _ => None,
        }
    }

    pub fn float(&mut self) -> Option<f32> {
        match self.next()? {
            Token::Float(v) => Some(v),
            Token::Int(v) => Some(v as f32),
            _ => None,
        }
    }

    pub fn floats<const N: usize>(&mut self) -> Option<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.float()?;
        }
        Some(out)
    }

    pub fn string(&mut self) -> Option<String> {
        match self.next()? {
            Token::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn expect(&mut self, token: Token) -> Option<()> {
        (self.next()? == token).then_some(())
    }

    /// Reads the `[count] {` that opens every list.
    pub fn list_header(&mut self) -> Option<usize> {
        self.expect(Token::LBracket)?;
        let count = self.int()?;
        self.expect(Token::RBracket)?;
        self.expect(Token::LCurly)?;
        Some(count as usize)
    }
}
