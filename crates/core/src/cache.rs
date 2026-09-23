use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub provider: String,
    pub source: String,
    pub target: String,
    pub text: String,
    pub glossary: u64,
}

pub struct MemoryCache {
    cap: usize,
    map: HashMap<CacheKey, String>,
    order: VecDeque<CacheKey>,
}

impl MemoryCache {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<String> {
        let value = self.map.get(key)?.clone();
        if let Some(index) = self.order.iter().position(|item| item == key) {
            if let Some(existing) = self.order.remove(index) {
                self.order.push_back(existing);
            }
        }
        Some(value)
    }

    pub fn insert(&mut self, key: CacheKey, value: String) {
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            let _ = self.get(&key);
            return;
        }
        while self.map.len() >= self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> CacheKey {
        CacheKey {
            provider: "echo".into(),
            source: "auto".into(),
            target: "zh".into(),
            text: text.into(),
            glossary: 0,
        }
    }

    #[test]
    fn evicts_the_least_recently_used_entry() {
        let mut cache = MemoryCache::new(2);
        cache.insert(key("a"), "甲".into());
        cache.insert(key("b"), "乙".into());
        assert_eq!(cache.get(&key("a")).as_deref(), Some("甲"));
        cache.insert(key("c"), "丙".into());
        assert_eq!(cache.get(&key("b")), None);
        assert_eq!(cache.get(&key("a")).as_deref(), Some("甲"));
        assert_eq!(cache.get(&key("c")).as_deref(), Some("丙"));
    }
}
