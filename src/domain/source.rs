#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Claude,
    Codex,
    All,
}

impl Source {
    pub fn from_filter(value: Option<&str>) -> Option<Self> {
        match value {
            Some("claude") => Some(Self::Claude),
            Some("codex") => Some(Self::Codex),
            Some("all") | None => Some(Self::All),
            _ => None,
        }
    }
}
