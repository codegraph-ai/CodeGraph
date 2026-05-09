// Test fixture: covers generics, async, modules — complement to basic.rs.

use std::collections::HashMap;

pub mod cache {
    use std::collections::HashMap;
    use std::hash::Hash;

    pub struct Cache<K, V>
    where
        K: Eq + Hash,
    {
        store: HashMap<K, V>,
    }

    impl<K, V> Cache<K, V>
    where
        K: Eq + Hash,
    {
        pub fn new() -> Self {
            Self { store: HashMap::new() }
        }

        pub fn insert(&mut self, key: K, value: V) {
            self.store.insert(key, value);
        }

        pub fn get(&self, key: &K) -> Option<&V> {
            self.store.get(key)
        }
    }
}

pub fn map_collect<T, U, F>(items: Vec<T>, f: F) -> Vec<U>
where
    F: Fn(T) -> U,
{
    items.into_iter().map(f).collect()
}

pub fn invert<K, V>(map: HashMap<K, V>) -> HashMap<V, K>
where
    V: Eq + std::hash::Hash,
{
    map.into_iter().map(|(k, v)| (v, k)).collect()
}

pub use cache::Cache;
