use ahash::RandomState;
use futures_lite::Future;
use std::pin::Pin;

pub use ahash::AHasher;

pub type BoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type HashMap<K, V> = std::collections::HashMap<K, V, RandomState>;
pub type HashSet<K> = std::collections::HashSet<K, RandomState>;

pub trait HashMapExt {
    fn with_capacity(cap: usize) -> Self;
}

impl<K, V> HashMapExt for HashMap<K, V> {
    fn with_capacity(cap: usize) -> Self {
        HashMap::with_capacity_and_hasher(cap, RandomState::default())
    }
}
