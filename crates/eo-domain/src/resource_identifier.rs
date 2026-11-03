//! `userRbacRole.resourceIdentifier` 的建構與解析（來源：`common/resourceIdentifier.ts`）。
//!
//! 格式 `<type>:<id>`，type 由 plugin 在執行期登記。TS 用模組層的全域 Set；
//! 這裡改成呼叫端持有的 registry（純邏輯不放全域可變狀態），語意相同。

use std::collections::BTreeSet;

use thiserror::Error;

const SEPARATOR: char = ':';

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResourceIdentifierError {
    #[error("Invalid resource type \"{0}\": must match /^[a-zA-Z][a-zA-Z0-9]*$/ (camelCase, alphanumeric)")]
    InvalidType(String),
    #[error("Unknown resource type \"{ty}\". Register it via plugin.getRbacSeeds().resourceTypes first. Known: [{known}]")]
    UnknownType { ty: String, known: String },
    #[error("Resource id must be non-empty (type={0})")]
    EmptyId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedResourceIdentifier {
    pub resource_type: String,
    pub id: String,
}

fn is_valid_type(t: &str) -> bool {
    let mut chars = t.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric())
}

#[derive(Debug, Clone, Default)]
pub struct ResourceTypeRegistry {
    known: BTreeSet<String>,
}

impl ResourceTypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登記一個 resource type。冪等。
    pub fn register(&mut self, t: &str) -> Result<(), ResourceIdentifierError> {
        if !is_valid_type(t) {
            return Err(ResourceIdentifierError::InvalidType(t.into()));
        }
        self.known.insert(t.into());
        Ok(())
    }

    pub fn register_all(&mut self, types: &[&str]) -> Result<(), ResourceIdentifierError> {
        types.iter().try_for_each(|t| self.register(t))
    }

    pub fn is_known(&self, t: &str) -> bool {
        self.known.contains(t)
    }

    /// 已登記的 type（排序後）。
    pub fn known_types(&self) -> Vec<String> {
        self.known.iter().cloned().collect()
    }

    /// 建 `<type>:<id>`。type 沒登記或 id 是空的就報錯。
    pub fn build(&self, t: &str, id: &str) -> Result<String, ResourceIdentifierError> {
        if !self.is_known(t) {
            return Err(ResourceIdentifierError::UnknownType { ty: t.into(), known: self.known_types().join(", ") });
        }
        if id.is_empty() {
            return Err(ResourceIdentifierError::EmptyId(t.into()));
        }
        Ok(format!("{t}{SEPARATOR}{id}"))
    }

    /// 解析；格式不對或 type 沒登記都回 None。空字串（非 resource 授權）也是 None。
    pub fn parse(&self, identifier: &str) -> Option<ParsedResourceIdentifier> {
        let (t, id) = identifier.split_once(SEPARATOR)?;
        if t.is_empty() || id.is_empty() || !self.is_known(t) {
            return None;
        }
        Some(ParsedResourceIdentifier { resource_type: t.into(), id: id.into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_roundtrip() {
        let mut reg = ResourceTypeRegistry::new();
        reg.register("ryxisAgent").unwrap();
        reg.register("ryxisAgent").unwrap();
        assert!(matches!(reg.register("bad-type"), Err(ResourceIdentifierError::InvalidType(_))));
        assert_eq!(reg.build("ryxisAgent", "12").unwrap(), "ryxisAgent:12");
        assert!(matches!(reg.build("nope", "1"), Err(ResourceIdentifierError::UnknownType { .. })));
        assert!(matches!(reg.build("ryxisAgent", ""), Err(ResourceIdentifierError::EmptyId(_))));
        assert_eq!(
            reg.parse("ryxisAgent:a:b"),
            Some(ParsedResourceIdentifier { resource_type: "ryxisAgent".into(), id: "a:b".into() })
        );
        assert_eq!(reg.parse(""), None);
        assert_eq!(reg.parse(":1"), None);
        assert_eq!(reg.parse("ryxisAgent:"), None);
        assert_eq!(reg.parse("other:1"), None);
        assert_eq!(reg.known_types(), vec!["ryxisAgent".to_string()]);
    }
}
