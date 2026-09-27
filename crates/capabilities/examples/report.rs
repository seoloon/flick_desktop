//! Prints the capability report as JSON: `cargo run -p oneshot-capabilities --example report`
fn main() {
    let report = oneshot_capabilities::CapabilityManager::new().refresh();
    println!("{}", serde_json::to_string_pretty(&*report).expect("serializable"));
}
