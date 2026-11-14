use super::models::Board;
use crate::core::error::AppError;
use reqwest::Url;

pub struct SearchBuilder<'a> {
    base: &'a str,
    board: Board,
    query: &'a str,
    page: u32,
    page_size: u32,
}

impl<'a> SearchBuilder<'a> {
    pub fn new(base: &'a str) -> Self {
        Self {
            base,
            board: Board::All,
            query: "",
            page: 1,
            page_size: 24,
        }
    }

    pub fn board(mut self, board: Board) -> Self {
        self.board = board;
        self
    }

    pub fn query(mut self, query: &'a str) -> Self {
        self.query = query;
        self
    }

    pub fn page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    pub fn page_size(mut self, page_size: u32) -> Self {
        self.page_size = page_size;
        self
    }

    pub fn build(self) -> Result<Url, AppError> {
        let mut url = Url::parse(&format!(
            "{}/api/v2/package/search/skills/{}",
            self.base,
            urlencoding::encode(self.query.trim())
        ))
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("scope", "all")
            .append_pair("order_by", self.board.order_by())
            .append_pair("page", &self.page.to_string())
            .append_pair("page_size", &self.page_size.to_string());
        Ok(url)
    }
}
