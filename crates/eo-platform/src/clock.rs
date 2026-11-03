//! 時間來源。產品碼一律從這裡取「現在」，測試可以換成固定時間。

use chrono::{DateTime, NaiveDate, Utc};
use std::sync::RwLock;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    /// 「今天」以台灣時間（UTC+8）計：代理效期、業務日期都用這個。
    fn today(&self) -> NaiveDate {
        (self.now() + chrono::Duration::hours(8)).date_naive()
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// 測試用：可以撥的時鐘。
pub struct ManualClock(RwLock<DateTime<Utc>>);

impl ManualClock {
    pub fn new(t: DateTime<Utc>) -> Self {
        Self(RwLock::new(t))
    }
    pub fn set(&self, t: DateTime<Utc>) {
        *self.0.write().unwrap() = t;
    }
    pub fn advance(&self, d: chrono::Duration) {
        let mut g = self.0.write().unwrap();
        *g += d;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.read().unwrap()
    }
}
