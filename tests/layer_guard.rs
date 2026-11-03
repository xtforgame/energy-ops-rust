//! 依賴方向：eo-domain 不碰 IO 也不依賴其他 eo crate；eo-platform 不依賴 eo-energy。

fn manifest(path: &str) -> String {
    std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn deps_section(m: &str) -> String {
    let start = m.find("[dependencies]").expect("[dependencies]");
    let rest = &m[start + "[dependencies]".len()..];
    let end = rest.find("\n[").unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn domain_has_no_io_or_internal_deps() {
    let d = deps_section(&manifest("crates/eo-domain/Cargo.toml"));
    for banned in ["eo-platform", "eo-energy", "sqlx", "actix", "tokio", "reqwest"] {
        assert!(!d.contains(banned), "eo-domain must not depend on {banned}");
    }
}

#[test]
fn platform_does_not_depend_on_energy() {
    let d = deps_section(&manifest("crates/eo-platform/Cargo.toml"));
    assert!(!d.contains("eo-energy"), "eo-platform must not depend on eo-energy");
}

#[test]
fn domain_sources_do_not_use_io_crates() {
    let dir = format!("{}/crates/eo-domain/src", env!("CARGO_MANIFEST_DIR"));
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|x| x == "rs").unwrap_or(false) {
                let src = std::fs::read_to_string(&path).unwrap();
                for banned in ["sqlx::", "actix_web::", "tokio::", "reqwest::", "std::fs::", "std::net::"] {
                    assert!(!src.contains(banned), "{} uses {banned}", path.display());
                }
            }
        }
    }
}
