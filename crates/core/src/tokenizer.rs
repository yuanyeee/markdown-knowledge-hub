//! Fixed-width Unicode ngrams with real character positions for literal phrases.
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
#[derive(Clone)]
pub struct PositionedNgram(pub usize);
pub struct Stream<'a> {
    text: &'a str,
    chars: std::iter::Enumerate<std::str::CharIndices<'a>>,
    width: usize,
    token: Token,
}
impl Tokenizer for PositionedNgram {
    type TokenStream<'a> = Stream<'a>;
    fn token_stream<'a>(&'a mut self, text: &'a str) -> Stream<'a> {
        Stream {
            text,
            chars: text.char_indices().enumerate(),
            width: self.0,
            token: Token::default(),
        }
    }
}
impl TokenStream for Stream<'_> {
    fn advance(&mut self) -> bool {
        let Some((position, (start, _))) = self.chars.next() else {
            return false;
        };
        let mut end = start;
        let mut count = 0;
        for c in self.text[start..].chars().take(self.width) {
            end += c.len_utf8();
            count += 1;
        }
        if count < self.width {
            return false;
        }
        self.token.position = position;
        self.token.position_length = 1;
        self.token.offset_from = start;
        self.token.offset_to = end;
        self.token.text.clear();
        self.token.text.push_str(&self.text[start..end]);
        true
    }
    fn token(&self) -> &Token {
        &self.token
    }
    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}
