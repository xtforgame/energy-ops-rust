// migration 檔編進 binary；資料夾變了要重編。
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
