//! serde 小工具：分得出「沒給這個鍵」與「給了 null」。
//!
//! 搭配 `#[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]`：
//! `None` ＝ 鍵不存在，`Some(None)` ＝ 明確給 null。

use serde::{Deserialize, Deserializer};

pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}
