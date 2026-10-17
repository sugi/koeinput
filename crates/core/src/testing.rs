use crate::recognizer::{Recognizer, RecognizerError};
use std::sync::atomic::{AtomicUsize, Ordering};

type DecodeFn = dyn Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync;

pub struct FakeRecognizer {
    f: Box<DecodeFn>,
    calls: AtomicUsize,
}

impl FakeRecognizer {
    pub fn new(f: impl Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync + 'static) -> Self {
        Self { f: Box::new(f), calls: AtomicUsize::new(0) }
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Recognizer for FakeRecognizer {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.f)(samples)
    }
}
