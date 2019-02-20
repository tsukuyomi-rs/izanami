#[derive(Debug, Default)]
pub struct SizeHint {
    lower: u64,
    upper: Option<u64>,
}

impl SizeHint {
    pub fn new() -> Self {
        Self::default()
    }

    #[doc(hidden)]
    pub fn exact(len: u64) -> Self {
        Self {
            lower: len,
            upper: Some(len),
        }
    }

    pub fn lower(&self) -> u64 {
        self.lower
    }

    pub fn upper(&self) -> Option<u64> {
        self.upper
    }

    pub fn set_lower(&mut self, value: u64) {
        assert!(value <= self.upper.unwrap_or(std::u64::MAX));
        self.lower = value;
    }

    pub fn set_upper(&mut self, value: u64) {
        assert!(value >= self.lower);
        self.upper = Some(value);
    }
}
