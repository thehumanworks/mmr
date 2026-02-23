#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pagination {
    pub limit: Option<usize>,
    pub offset: usize,
}

impl Pagination {
    pub fn new(limit: Option<usize>, offset: usize) -> Self {
        Self { limit, offset }
    }
}
